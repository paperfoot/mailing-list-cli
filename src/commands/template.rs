use crate::cli::{
    TemplateAction, TemplateCreateArgs, TemplateInspectArgs, TemplateLintArgs, TemplatePreviewArgs,
    TemplateRenderArgs, TemplateRmArgs, TemplateShowArgs,
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
const SCAFFOLD: &str = r##"<!--
  mailing-list-cli template — quick reference (v0.2.3+).

  Supported substitution syntax (nothing else, agents take note):
    {{ var }}         — HTML-escaped substitution for contact fields
    {{{ var }}}       — raw (unescaped) substitution. ALLOWLISTED to exactly
                         {{{ unsubscribe_link }}} and {{{ physical_address_footer }}}
                         because those are injected as HTML at send time.
                         Using triple-brace for anything else is an XSS risk
                         and a lint error.
    {{#if name}}...{{/if}}          — render block if merge field is truthy
    {{#unless name}}...{{/unless}}  — render block if merge field is absent/falsy
    Conditionals nest and can be paired (see body below).

  NOT supported (don't reach for them — they'll render literally or error):
    - Handlebars helpers other than `if` / `unless` (no `{{#each}}`, no `{{#with}}`)
    - MJML tags (no `<mj-*>` — v0.2 deleted the MJML stack)
    - Partials, block helpers, custom functions
    - Liquid / Jinja / Django syntax

  How this gets sent:
    - `--subject` itself can contain `{{ var }}` tags ("Hi, {{ first_name }}").
    - Any unresolved `{{ var }}` at send time is a HARD FAIL (broadcast aborts,
      status → failed). `{{#if}}` branches with absent data are treated as
      falsy and do not count as unresolved.
    - `broadcast send` uses strict render mode; `template preview` uses
      lenient mode and injects realistic stubs for the two allowlisted
      triple-brace tokens so the preview HTML is viewable.

  The 6 lint rules (`template lint <name>`):
    1. body contains `{{{ unsubscribe_link }}}`
    2. body contains `{{{ physical_address_footer }}}`
    3. final HTML < 102 KB (Gmail clip limit)
    4. no `<script>`, `<iframe>`, `<object>`, `<embed>`, `<form>` tags
    5. no triple-brace used outside the two allowlist names
    6. structural sanity (subject non-empty, etc.)

  Email design rules for agents:
    - If you are starting from a design handoff, React/JSX file, canvas export,
      or webpage prototype, run `template inspect --from-file <path>` first.
      A `browser_prototype_needs_conversion` verdict means it is design
      direction only; convert it before `template create`.
    - Build email like email, not like a webpage. Use table-based wrappers,
      centered content, inline cell padding, and inline styles.
    - Keep the content width around 600-640 px with visible outer padding.
    - Style every text link inline. Unstyled links turn default blue/purple in
      Gmail and look unfinished.
    - Avoid semantic layout tags such as <main>, <section>, <article>,
      <header>, and <footer>; they are browser-oriented and fragile in email.
    - Use restrained type, clear paragraph spacing, a single obvious CTA, and
      a readable compliance footer.
    - Before a real send, run template lint, inspect template preview output
      including plain.txt, then send broadcast preview to an internal address.

  Delete this comment once you're done reading it — the final sent email
  carries whatever is in the source.
-->
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{{ NAME }}</title>
</head>
<body style="margin:0;padding:0;background:#f3f0e8;color:#111827;font-family:Arial,Helvetica,sans-serif;">
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="width:100%;border-collapse:collapse;background:#f3f0e8;">
    <tr>
      <td align="center" style="padding:32px 16px;">
        <table role="presentation" width="640" cellpadding="0" cellspacing="0" border="0" style="width:100%;max-width:640px;border-collapse:collapse;background:#ffffff;border:1px solid #e5e1d8;">
          <tr>
            <td style="padding:34px 32px 30px;">
              <p style="margin:0 0 22px;font-size:12px;line-height:1.4;letter-spacing:0.08em;text-transform:uppercase;color:#6b7280;">
                Newsletter
              </p>
              <h1 style="margin:0 0 18px;font-size:28px;line-height:1.18;color:#111827;font-weight:700;">
                Hi {{ first_name }}
              </h1>
              <p style="margin:0 0 16px;font-size:16px;line-height:1.62;color:#1f2937;">
                Replace this body with your content. Write normal HTML with
                table-based wrappers and inline styles for best client
                compatibility. Avoid semantic layout tags like <code>&lt;main&gt;</code>
                in production email.
              </p>
              {{#if referral_code}}
              <p style="margin:0 0 16px;font-size:16px;line-height:1.62;color:#1f2937;">
                You used referral code <strong>{{ referral_code }}</strong>, so your first order ships free.
              </p>
              {{/if}}
              {{#unless referral_code}}
              <p style="margin:0 0 16px;font-size:16px;line-height:1.62;color:#1f2937;">
                No referral code this time — welcome aboard anyway.
              </p>
              {{/unless}}
              <p style="margin:26px 0 28px;">
                <a href="https://example.com/cta"
                   style="display:inline-block;padding:12px 18px;background:#111827;color:#ffffff;text-decoration:none;border-radius:4px;font-size:14px;line-height:1.2;font-weight:700">
                  Call to action
                </a>
              </p>
              <p style="margin:0 0 18px;font-size:15px;line-height:1.6;color:#1f2937;">
                Use inline link styles for text links too:
                <a href="https://example.com" style="color:#111827;text-decoration:underline;">example link</a>.
              </p>
              <p style="margin:32px 0 0;padding-top:18px;border-top:1px solid #e5e7eb;font-size:12px;line-height:1.5;color:#6b7280;text-align:left">
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
        TemplateAction::Inspect(args) => inspect(format, args),
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
            "scaffolded": args.from_file.is_none(),
            "design_assessment": inspect_template_source("created_template", &source, subject)
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

fn inspect(format: Format, args: TemplateInspectArgs) -> Result<(), AppError> {
    if args.name.is_some() && args.from_file.is_some() {
        return Err(AppError::BadInput {
            code: "template_inspect_ambiguous_source".into(),
            message: "inspect accepts either a stored template name or --from-file, not both".into(),
            suggestion: "Use `mailing-list-cli template inspect <name>` or `mailing-list-cli template inspect --from-file design.html`".into(),
        });
    }

    let (source_label, subject, source) = match (&args.name, &args.from_file) {
        (Some(name), None) => {
            let db = Db::open()?;
            let t = db
                .template_get_by_name(name)?
                .ok_or_else(|| AppError::BadInput {
                    code: "template_not_found".into(),
                    message: format!("no template named '{name}'"),
                    suggestion: "Run `mailing-list-cli template ls`".into(),
                })?;
            (format!("template:{name}"), t.subject, t.html_source)
        }
        (None, Some(path)) => {
            let source = std::fs::read_to_string(path).map_err(|e| AppError::BadInput {
                code: "template_file_read_failed".into(),
                message: format!("could not read {}: {e}", path.display()),
                suggestion: "Check the file path and permissions".into(),
            })?;
            let subject = args
                .subject
                .as_deref()
                .unwrap_or("(inspection subject)")
                .to_string();
            (format!("file:{}", path.display()), subject, source)
        }
        (None, None) => {
            return Err(AppError::BadInput {
                code: "template_inspect_source_required".into(),
                message: "inspect requires a stored template name or --from-file".into(),
                suggestion: "Use `mailing-list-cli template inspect <name>` or `mailing-list-cli template inspect --from-file design.html`".into(),
            });
        }
        (Some(_), Some(_)) => unreachable!(),
    };

    let assessment = inspect_template_source(&source_label, &source, &subject);
    let verdict = assessment
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    output::success(
        format,
        &format!("template inspection: {verdict}"),
        assessment,
    );
    Ok(())
}

fn inspect_template_source(source_label: &str, html_source: &str, subject: &str) -> Value {
    let rendered = template::lint(html_source, subject);
    let lower = html_source.to_ascii_lowercase();
    let source_label_lower = source_label.to_ascii_lowercase();
    let mut design_findings = Vec::new();

    let mut push_design = |severity: &str, code: &str, message: &str, hint: &str| {
        design_findings.push(json!({
            "severity": severity,
            "code": code,
            "message": message,
            "hint": hint
        }));
    };

    let looks_like_js_source = source_label_lower.ends_with(".jsx")
        || source_label_lower.ends_with(".tsx")
        || source_label_lower.ends_with(".js")
        || source_label_lower.ends_with(".ts")
        || lower.contains("import react")
        || lower.contains("from 'react'")
        || lower.contains("from \"react\"")
        || lower.contains("reactdom")
        || lower.contains("createroot(");

    if looks_like_js_source {
        push_design(
            "error",
            "browser_or_jsx_source",
            "source looks like a browser/React/JSX prototype, not send-ready email HTML",
            "Extract the visual/content direction, then rewrite it as standalone table-based HTML with inline styles before `template create`.",
        );
    }

    if lower.contains("<script")
        || lower.contains("type=\"text/babel")
        || lower.contains("type='text/babel")
    {
        push_design(
            "error",
            "browser_script_dependency",
            "source depends on JavaScript or Babel",
            "Email clients strip scripts. Remove JS entirely and express the final layout as static HTML tables with inline styles.",
        );
    }

    if lower.contains("<link") && lower.contains("stylesheet") {
        push_design(
            "warning",
            "external_stylesheet",
            "source references an external stylesheet",
            "Inline the required styles onto the elements/cells that need them; most email clients strip or rewrite external CSS.",
        );
    }

    if lower.contains("<style") || lower.contains("@import") {
        push_design(
            "warning",
            "style_block",
            "source relies on a style block or CSS import",
            "For production sends, move critical styles inline. Gmail and other clients can drop or rewrite head CSS.",
        );
    }

    if lower.contains("display:flex")
        || lower.contains("display: flex")
        || lower.contains("display:grid")
        || lower.contains("display: grid")
    {
        push_design(
            "warning",
            "browser_layout_css",
            "source uses browser layout CSS such as flex or grid",
            "Rebuild complex rows/columns with presentation tables and inline cell padding for email-client compatibility.",
        );
    }

    if html_source.len() > 1_500 && !lower.contains("<table") {
        push_design(
            "warning",
            "no_table_layout",
            "rich template has no table-based layout",
            "For designed newsletters, use a 100% outer presentation table and a centered 600-640px inner table.",
        );
    }

    if lower.contains("class=") && !lower.contains("style=") {
        push_design(
            "warning",
            "class_styles_without_inline_styles",
            "source appears class-driven rather than inline-styled",
            "Classes alone are fragile in email. Keep important visual styles inline even if classes remain for tooling.",
        );
    }

    let design_errors = design_findings
        .iter()
        .filter(|f| f.get("severity").and_then(|v| v.as_str()) == Some("error"))
        .count();
    let design_warnings = design_findings
        .iter()
        .filter(|f| f.get("severity").and_then(|v| v.as_str()) == Some("warning"))
        .count();

    let conversion_required = design_errors > 0;
    let ready_to_send = !conversion_required && rendered.error_count() == 0;
    let verdict = if conversion_required {
        "browser_prototype_needs_conversion"
    } else if rendered.error_count() > 0 {
        "not_send_ready"
    } else if rendered.warning_count() + design_warnings > 0 {
        "email_candidate_with_warnings"
    } else {
        "email_ready"
    };

    let recommended_next_steps = if conversion_required {
        vec![
            "Do not import/send this file directly.".to_string(),
            "Convert the design into static email HTML: 100% outer table, centered 600-640px inner table, inline styles, styled links, and no scripts/imports.".to_string(),
            "Add `{{{ unsubscribe_link }}}` and `{{{ physical_address_footer }}}` near the footer.".to_string(),
            "Run `template inspect --from-file converted.html`, then `template create`, `template lint`, `template preview`, `broadcast preview`, `broadcast send --dry-run`, and only then `broadcast send --confirm`.".to_string(),
        ]
    } else if !ready_to_send {
        vec![
            "Fix lint errors before sending.".to_string(),
            "Run `template lint` and inspect the returned findings.".to_string(),
            "Preview the rendered HTML and plain-text fallback before broadcast preview or send."
                .to_string(),
        ]
    } else {
        vec![
            "Run `template create --from-file` if this is not stored yet.".to_string(),
            "Run `template lint`, inspect `template preview` output including plain.txt, then send `broadcast preview` to an internal address.".to_string(),
            "Use `broadcast send --dry-run` before the required `broadcast send --confirm`.".to_string(),
        ]
    };

    json!({
        "source": source_label,
        "subject": subject,
        "verdict": verdict,
        "ready_to_send": ready_to_send,
        "conversion_required": conversion_required,
        "size_bytes": html_source.len(),
        "lint_errors": rendered.error_count(),
        "lint_warnings": rendered.warning_count(),
        "design_errors": design_errors,
        "design_warnings": design_warnings,
        "lint_findings": rendered.findings,
        "design_findings": design_findings,
        "recommended_next_steps": recommended_next_steps,
        "email_design_contract": [
            "standalone static HTML only; no JavaScript, React runtime, Babel, external CSS, iframe/form/object/embed",
            "100% outer presentation table plus centered 600-640px inner table for designed newsletters",
            "inline styles on layout cells, text, CTA anchors, and ordinary text links",
            "visible unsubscribe and physical-address footer placeholders",
            "plain-text fallback must keep CTA and unsubscribe URLs visible"
        ]
    })
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
    // Default: inject realistic stubs for the two reserved triple-brace names
    // so the output is viewable HTML and matches what `template preview`
    // produces. Pass `--raw` to leave them literal (for piping to a downstream
    // substituter). Note: the stubs are inline elements (`<a>` and `<span>`)
    // so they're safe to place inside a `<p>` or other inline context.
    if !args.raw {
        if let Value::Object(map) = &mut data {
            map.entry(String::from("unsubscribe_link")).or_insert(json!(
                "<a href=\"https://hooks.example.invalid/u/PLACEHOLDER_UNSUBSCRIBE_TOKEN\" target=\"_blank\" rel=\"nofollow\" data-utm=\"off\" style=\"color:#4b5563;text-decoration:underline\">Unsubscribe</a>"
            ));
            map.entry(String::from("physical_address_footer")).or_insert(json!(
                "<span style=\"color:#666;font-size:11px\">Your Company Name · 123 Example Street · City, ST 00000</span>"
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
            "<a href=\"https://hooks.example.invalid/u/PLACEHOLDER_UNSUBSCRIBE_TOKEN\" target=\"_blank\" rel=\"nofollow\" data-utm=\"off\" style=\"color:#4b5563;text-decoration:underline\">Unsubscribe</a>"
        ));
        // Inline <span> stub so it's safe to inject inside a `<p>` or any other
        // inline context in the template — matches the broadcast pipeline's
        // footer HTML shape in v0.2.3+.
        map.entry(String::from("physical_address_footer")).or_insert(json!(
            "<span style=\"color:#666;font-size:11px\">Your Company Name · 123 Example Street · City, ST 00000</span>"
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
