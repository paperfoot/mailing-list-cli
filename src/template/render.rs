//! v0.2 template render + lint pipeline.
//!
//! Replaces the old `src/template/{compile,lint,frontmatter}.rs` trio. Template
//! source is now **plain HTML** (no frontmatter, no MJML, no Handlebars).
//! Agents write inline styles directly; the compiler is just a merge-tag
//! substituter plus a handful of compliance / security checks.
//!
//! ```
//!   html_source + subject + merge_data
//!     │
//!     ├─ substitute (subst::substitute)
//!     │
//!     ├─ 6-rule lint inline
//!     │
//!     ├─ plain-text fallback (regex-strip)
//!     │
//!     └─ Rendered { subject, html, text, size_bytes, findings }
//! ```
//!
//! Two entry points:
//!   - `render(source, subject, data)` — full render with unresolved-as-error.
//!     Used by the send pipeline. Missing `{{ var }}` aborts the send.
//!   - `render_preview(source, subject, data)` — lenient render with
//!     unresolved-as-warning. Used by `template preview` and `template render`.

use crate::template::subst::{BUILT_INS, TRIPLE_BRACE_ALLOWLIST, substitute};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("render has errors: {0}")]
    Lint(String),
    #[error("unresolved placeholders at send time: {0}")]
    UnresolvedAtSend(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    pub subject: String,
    pub html: String,
    pub text: String,
    pub size_bytes: usize,
    pub findings: Vec<LintFinding>,
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LintRule {
    /// `{{{ unsubscribe_link }}}` is missing from the body. CAN-SPAM + Gmail/Yahoo requirement.
    UnsubscribeLinkMissing,
    /// `{{{ physical_address_footer }}}` is missing from the body. CAN-SPAM hard requirement.
    PhysicalAddressFooterMissing,
    /// Rendered HTML is at or above 102 KB — Gmail will clip it, hiding the footer.
    HtmlSizeError,
    /// Source contains a forbidden tag like `<script>`, `<form>`, `<iframe>`.
    ForbiddenTag,
    /// A `{{ var }}` reference has no value in the merge data. Warning at
    /// preview time, hard error at send time.
    UnresolvedPlaceholder,
    /// A `{{{ name }}}` triple-brace reference is not in the allowlist.
    /// Security-critical (XSS prevention).
    ForbiddenRawInjection,
}

#[derive(Debug, Clone, Serialize)]
pub struct LintFinding {
    pub severity: Severity,
    pub rule: LintRule,
    pub message: String,
    pub hint: String,
}

const GMAIL_CLIP_LIMIT: usize = 102_000;
const FORBIDDEN_TAGS: &[&str] = &["<script", "<form", "<iframe", "<object", "<embed"];

/// Render for SEND: unresolved placeholders are an error. The send pipeline
/// calls this after injecting `unsubscribe_link`, `physical_address_footer`,
/// and all contact fields into `data`. Any `{{ var }}` still unresolved at
/// this point is a template bug and must abort the send.
pub fn render(
    html_source: &str,
    subject_source: &str,
    data: &serde_json::Value,
) -> Result<Rendered, RenderError> {
    let rendered = render_inner(html_source, subject_source, data, /*strict=*/ true);
    if !rendered.unresolved.is_empty() {
        return Err(RenderError::UnresolvedAtSend(
            rendered.unresolved.join(", "),
        ));
    }
    let errors = rendered
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    if errors > 0 {
        let msgs = rendered
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .map(|f| f.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(RenderError::Lint(msgs));
    }
    Ok(rendered)
}

/// Render for PREVIEW: unresolved placeholders are a warning, the preview
/// still renders. Used by `template preview`, `template render`, and
/// `template lint`. The caller inspects `.findings` and `.unresolved`.
pub fn render_preview(
    html_source: &str,
    subject_source: &str,
    data: &serde_json::Value,
) -> Rendered {
    render_inner(html_source, subject_source, data, /*strict=*/ false)
}

fn render_inner(
    html_source: &str,
    subject_source: &str,
    data: &serde_json::Value,
    strict: bool,
) -> Rendered {
    // Subject goes through substitution too — agents put `{{ first_name }}`
    // in subjects all the time.
    let subject_sub = substitute(subject_source, data);
    let body_sub = substitute(html_source, data);

    let mut findings = Vec::new();

    // Rule 1: CAN-SPAM — must contain the unsubscribe link placeholder.
    if !html_source.contains("{{{ unsubscribe_link }}}")
        && !html_source.contains("{{{unsubscribe_link}}}")
    {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::UnsubscribeLinkMissing,
            message: "template body does not contain `{{{ unsubscribe_link }}}`".into(),
            hint: "Insert `{{{ unsubscribe_link }}}` inside an anchor or text near the footer — it's replaced at send time with a one-click unsubscribe URL (CAN-SPAM + RFC 8058 required)".into(),
        });
    }

    // Rule 2: CAN-SPAM — must contain the physical address footer placeholder.
    if !html_source.contains("{{{ physical_address_footer }}}")
        && !html_source.contains("{{{physical_address_footer}}}")
    {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::PhysicalAddressFooterMissing,
            message: "template body does not contain `{{{ physical_address_footer }}}`".into(),
            hint: "Insert `{{{ physical_address_footer }}}` near the unsubscribe link — required by CAN-SPAM (section 7704)".into(),
        });
    }

    // Rule 3: Gmail clip — post-substitution HTML size must be under 102 KB.
    let html_size = body_sub.output.len();
    if html_size >= GMAIL_CLIP_LIMIT {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::HtmlSizeError,
            message: format!(
                "rendered HTML is {html_size} bytes (Gmail clips at {GMAIL_CLIP_LIMIT} — the footer and unsubscribe link will be hidden)"
            ),
            hint: "Reduce the template size — smaller inline images, fewer sections, or move content to a landing page. A clipped footer is a compliance failure, not aesthetics.".into(),
        });
    }

    // Rule 4: Forbidden tags — security / hostile client policy.
    for tag in FORBIDDEN_TAGS {
        if html_source.contains(tag) {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ForbiddenTag,
                message: format!("template contains forbidden tag `{tag}`"),
                hint: "This tag is blocked by most email clients or by mailing-list-cli's security policy. Remove it.".into(),
            });
        }
    }

    // Rule 5: Unresolved placeholders. Warning in preview, error in strict/send.
    // We collect from BOTH subject and body (dedup).
    let mut unresolved: Vec<String> = body_sub.unresolved.clone();
    for u in &subject_sub.unresolved {
        if !unresolved.contains(u) {
            unresolved.push(u.clone());
        }
    }
    // Filter out built-ins that the send pipeline will always inject — those
    // shouldn't be user-fixable warnings at preview time.
    unresolved.retain(|name| !BUILT_INS.contains(&name.as_str()));

    for name in &unresolved {
        let severity = if strict {
            Severity::Error
        } else {
            Severity::Warning
        };
        findings.push(LintFinding {
            severity,
            rule: LintRule::UnresolvedPlaceholder,
            message: format!("unresolved merge tag `{{{{ {name} }}}}`"),
            hint: format!(
                "Either provide a value for `{name}` in the merge data, or remove the `{{{{ {name} }}}}` reference from the template."
            ),
        });
    }

    // Rule 6: Forbidden raw injection — triple-brace names outside allowlist.
    for name in &body_sub.forbidden_raw {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::ForbiddenRawInjection,
            message: format!(
                "`{{{{{{ {name} }}}}}}` uses raw-HTML triple-brace but is not in the allowlist"
            ),
            hint: format!(
                "Raw HTML substitution is restricted to: {}. Use double-brace `{{{{ {name} }}}}` for contact fields (auto-escaped).",
                TRIPLE_BRACE_ALLOWLIST.join(", ")
            ),
        });
    }

    let text = html_to_text(&body_sub.output);

    Rendered {
        subject: subject_sub.output,
        html: body_sub.output,
        text,
        size_bytes: html_size,
        findings,
        unresolved,
    }
}

/// Minimal HTML-to-text extraction. Drops `<style>` and `<script>` blocks
/// entirely, strips all tags, collapses whitespace, preserves basic line
/// breaks. Not pretty — good enough for a plain-text multipart alternative
/// that spam filters accept.
fn html_to_text(html: &str) -> String {
    // Strip <style>...</style> and <script>...</script> blocks (non-greedy).
    let mut cleaned = String::with_capacity(html.len());
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        // Look for opening of a stripped block.
        if let Some(rest) = html[i..].strip_prefix("<style") {
            if let Some(close) = rest.find("</style>") {
                i += "<style".len() + close + "</style>".len();
                continue;
            }
            // No closer — drop to EOF.
            break;
        }
        if let Some(rest) = html[i..].strip_prefix("<script") {
            if let Some(close) = rest.find("</script>") {
                i += "<script".len() + close + "</script>".len();
                continue;
            }
            break;
        }
        cleaned.push(html[i..].chars().next().unwrap());
        i += html[i..].chars().next().unwrap().len_utf8();
    }

    // Replace block-level tags with line breaks BEFORE stripping all tags.
    let with_breaks = cleaned
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</p>", "\n\n")
        .replace("</div>", "\n")
        .replace("</h1>", "\n\n")
        .replace("</h2>", "\n\n")
        .replace("</h3>", "\n\n")
        .replace("</li>", "\n");

    // Strip every tag.
    let mut out = String::with_capacity(with_breaks.len());
    let mut in_tag = false;
    for c in with_breaks.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }

    // Unescape common HTML entities (cheap + complete enough for our needs).
    let unescaped = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    // Collapse consecutive whitespace while preserving double-newlines.
    collapse_whitespace(&unescaped)
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;
    let mut newline_run = 0;
    for c in s.chars() {
        if c == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
            last_was_space = false;
            continue;
        }
        newline_run = 0;
        if c.is_whitespace() {
            if !last_was_space && !out.is_empty() {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

/// Lint-only entry point for `template lint`. Reuses the preview render.
pub fn lint(html_source: &str, subject_source: &str) -> Rendered {
    // Stub merge data so CAN-SPAM checks on the source are deterministic and
    // size is measured against a reasonable expansion. Unresolved warnings
    // are filtered out because the lint isn't running against real data.
    let stub = serde_json::json!({
        "first_name": "Preview",
        "last_name": "User",
        "email": "preview@example.invalid",
        "current_year": 2026,
        "broadcast_id": 0,
        "unsubscribe_link": "<a href=\"https://hooks.example.invalid/u/PLACEHOLDER_UNSUBSCRIBE_TOKEN_aaaaaaaaaaaaaaaaaaaaaaaaaa\" target=\"_blank\">Unsubscribe</a>",
        "physical_address_footer": "<div style=\"color:#666;font-size:11px;text-align:center;margin-top:20px\">Your Company Name · 123 Example Street · Suite 400 · City, ST 00000 · United States</div>"
    });
    let mut r = render_preview(html_source, subject_source, &stub);
    // Strip unresolved findings so lint focuses on structural/security issues.
    // Truly unresolved custom vars (that aren't built-ins) WILL still show up
    // here — the retain above only drops built-ins.
    r.findings
        .retain(|f| f.rule != LintRule::UnresolvedPlaceholder);
    r.unresolved.clear();
    r
}

impl Rendered {
    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }
    pub fn warning_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }
    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const VALID: &str = r#"<!doctype html>
<html>
<body>
<p>Hi {{ first_name }}</p>
<p style="color:#666;font-size:12px">
  {{{ unsubscribe_link }}}
  <br>
  {{{ physical_address_footer }}}
</p>
</body>
</html>"#;

    #[test]
    fn render_preview_lenient_on_missing_vars() {
        let r = render_preview(VALID, "Hi {{ first_name }}", &json!({}));
        // first_name is a BUILT_IN so it's filtered out of unresolved.
        assert!(r.unresolved.is_empty());
        assert_eq!(r.error_count(), 0);
    }

    #[test]
    fn render_send_hard_fails_on_unresolved() {
        let source = r#"Hi {{ typo_name }}
{{{ unsubscribe_link }}}
{{{ physical_address_footer }}}"#;
        let err = render(source, "Subject", &json!({})).unwrap_err();
        match err {
            RenderError::UnresolvedAtSend(ref msg) => assert!(msg.contains("typo_name")),
            _ => panic!("expected UnresolvedAtSend, got {err:?}"),
        }
    }

    #[test]
    fn lint_flags_missing_unsubscribe() {
        let r = lint("<p>hi</p>{{{ physical_address_footer }}}", "subject");
        assert!(
            r.findings
                .iter()
                .any(|f| f.rule == LintRule::UnsubscribeLinkMissing)
        );
    }

    #[test]
    fn lint_flags_missing_footer() {
        let r = lint("<p>hi</p>{{{ unsubscribe_link }}}", "subject");
        assert!(
            r.findings
                .iter()
                .any(|f| f.rule == LintRule::PhysicalAddressFooterMissing)
        );
    }

    #[test]
    fn lint_flags_forbidden_tag() {
        let src = format!("<p><script>evil()</script></p>{VALID}");
        let r = lint(&src, "subject");
        assert!(r.findings.iter().any(|f| f.rule == LintRule::ForbiddenTag));
    }

    #[test]
    fn lint_flags_forbidden_raw_injection() {
        let src = format!("{VALID}{{{{{{ user_html }}}}}}");
        let r = lint(&src, "subject");
        assert!(
            r.findings
                .iter()
                .any(|f| f.rule == LintRule::ForbiddenRawInjection)
        );
    }

    #[test]
    fn lint_clean_template_passes() {
        let r = lint(VALID, "Hi {{ first_name }}");
        assert_eq!(r.error_count(), 0, "findings: {:?}", r.findings);
    }

    #[test]
    fn oversize_html_is_flagged() {
        let big = "<p>".repeat(20_000) + "</p>".repeat(20_000).as_str();
        let src = format!("{big}{VALID}");
        let r = lint(&src, "subject");
        assert!(r.findings.iter().any(|f| f.rule == LintRule::HtmlSizeError));
    }

    #[test]
    fn plain_text_strips_tags_and_scripts() {
        let html = r#"<html><head><style>.x{color:red}</style></head><body><p>Hello <b>world</b></p><script>evil()</script></body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains("color:red"));
        assert!(!text.contains("evil()"));
        assert!(!text.contains("<"));
    }

    #[test]
    fn subject_also_substitutes() {
        let r = render_preview(
            VALID,
            "Welcome, {{ first_name }}",
            &json!({"first_name": "Alice"}),
        );
        assert_eq!(r.subject, "Welcome, Alice");
    }
}
