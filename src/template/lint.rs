//! Template lint rules. See spec §7.3 for the authoritative list.

use crate::template::compile::{Rendered, compile_with_placeholders};
use crate::template::frontmatter::{FrontmatterError, ParsedTemplate, split_frontmatter};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LintRule {
    FrontmatterMissing,
    FrontmatterInvalid,
    MjmlParseFailed,
    UndeclaredVariable,
    UnusedVariable, // now an ERROR, not warning
    DangerousCss,
    HtmlSizeWarning, // 90 KB warn threshold
    HtmlSizeError,   // 102 KB Gmail clip error
    EmptyPlainText,
    SubjectTooLong,
    SubjectEmpty,
    UnsubscribeLinkMissing,
    PhysicalAddressFooterMissing,
    MjPreviewMissing,
    ForbiddenTag,         // <script>/<form>/<iframe>/<object>/<embed>/<mj-include>
    RawTableOutsideMjRaw, // now ERROR, not warning
    ImageMissingAlt,      // <mj-image> without alt
    ButtonMissingHref,    // <mj-button> without real href
    ForbiddenTripleBrace, // {{{ foo }}} where foo is not in allowlist
    ForbiddenHelper,      // {{#each}} or {{> partial}}
}

#[derive(Debug, Clone, Serialize)]
pub struct LintFinding {
    pub severity: Severity,
    pub rule: LintRule,
    pub message: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LintOutcome {
    pub findings: Vec<LintFinding>,
    pub error_count: usize,
    pub warning_count: usize,
}

impl LintOutcome {
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }
}

const GMAIL_CLIP_ERROR: usize = 102_000; // Gmail clips at 102 KB; clipping hides unsubscribe link
const GMAIL_CLIP_WARN: usize = 90_000; // Warn before we hit the cliff
const SUBJECT_MAX_LEN: usize = 70; // Codex FIX #6: was 100, tightened

const ALLOWED_TRIPLE_BRACE: &[&str] = &["unsubscribe_link", "physical_address_footer"];
const FORBIDDEN_TAGS: &[&str] = &[
    "<script",
    "<form",
    "<iframe",
    "<object",
    "<embed",
    "<mj-include",
];
const BUILT_INS: &[&str] = &[
    "first_name",
    "last_name",
    "email",
    "current_year",
    "broadcast_id",
    "unsubscribe_link",
    "physical_address_footer",
];

pub fn lint(source: &str) -> LintOutcome {
    let mut findings: Vec<LintFinding> = Vec::new();

    // 1. Frontmatter
    let parsed = match split_frontmatter(source) {
        Ok(p) => p,
        Err(e) => {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: match e {
                    FrontmatterError::Missing => LintRule::FrontmatterMissing,
                    _ => LintRule::FrontmatterInvalid,
                },
                message: format!("{e}"),
                hint: "Every template must start with a `---` delimited YAML frontmatter block declaring name, subject, and variables".into(),
            });
            return summarize(findings);
        }
    };
    let ParsedTemplate { schema, body } = parsed;

    // 2. Required presence checks on the body text
    if !body.contains("{{{ unsubscribe_link }}}") {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::UnsubscribeLinkMissing,
            message: "template body does not contain {{{ unsubscribe_link }}}".into(),
            hint: "Insert `{{{ unsubscribe_link }}}` inside an <mj-text> near the footer — it's replaced at send time with a one-click unsubscribe URL".into(),
        });
    }
    if !body.contains("{{{ physical_address_footer }}}") {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::PhysicalAddressFooterMissing,
            message: "template body does not contain {{{ physical_address_footer }}}".into(),
            hint: "Insert `{{{ physical_address_footer }}}` inside an <mj-text> near the unsubscribe link — required by CAN-SPAM".into(),
        });
    }

    // 3. Subject
    if schema.subject.is_empty() {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::SubjectEmpty,
            message: "subject is empty".into(),
            hint: "Add a subject line to the frontmatter".into(),
        });
    } else if schema.subject.len() > SUBJECT_MAX_LEN {
        findings.push(LintFinding {
            severity: Severity::Warning,
            rule: LintRule::SubjectTooLong,
            message: format!(
                "subject is {} chars (max recommended: {SUBJECT_MAX_LEN})",
                schema.subject.len()
            ),
            hint: "Long subjects are truncated on mobile — aim for < 50 chars when possible".into(),
        });
    }

    // 4. Mj-preview recommended (warning only)
    if !body.contains("<mj-preview>") {
        findings.push(LintFinding {
            severity: Severity::Warning,
            rule: LintRule::MjPreviewMissing,
            message: "template has no <mj-preview>".into(),
            hint: "Preview text in the inbox row dramatically affects open rates — add `<mj-preview>...</mj-preview>` to <mj-head>".into(),
        });
    }

    // 5. Dangerous CSS
    for pat in ["flex", "grid", "float:", "position:"] {
        if body.contains(pat) {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::DangerousCss,
                message: format!("body contains `{pat}` which breaks in Outlook desktop"),
                hint: "Use <mj-section>/<mj-column>/<mj-spacer> for layout instead of modern CSS"
                    .into(),
            });
        }
    }

    // 6. Forbidden tags (script/form/iframe/object/embed/mj-include)
    for tag in FORBIDDEN_TAGS {
        if body.contains(tag) {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ForbiddenTag,
                message: format!("template contains forbidden tag `{tag}`"),
                hint: "This tag is blocked by most email clients and/or by mailing-list-cli's security policy. Remove it.".into(),
            });
        }
    }

    // 7. Raw <table> or <div> outside <mj-raw> (now an error per Codex review)
    if body.contains("<table") && !body.contains("<mj-raw>") && !body.contains("<mj-table>") {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::RawTableOutsideMjRaw,
            message: "template contains raw `<table>` outside of `<mj-raw>` or `<mj-table>`".into(),
            hint: "Use `<mj-section>/<mj-column>` for layout or `<mj-table>` for tabular data"
                .into(),
        });
    }

    // 8. <mj-image> must have alt attribute
    for idx in find_tag_positions(&body, "<mj-image") {
        let tag_end = body[idx..].find('>').map(|n| idx + n).unwrap_or(body.len());
        let tag_slice = &body[idx..tag_end];
        if !tag_slice.contains("alt=") {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ImageMissingAlt,
                message: "`<mj-image>` missing `alt` attribute".into(),
                hint: "Add `alt=\"descriptive text\"` to every image. Screen readers and spam filters care.".into(),
            });
            break; // report once, not per image
        }
    }

    // 9. <mj-button> must have non-empty, non-`#` href
    for idx in find_tag_positions(&body, "<mj-button") {
        let tag_end = body[idx..].find('>').map(|n| idx + n).unwrap_or(body.len());
        let tag_slice = &body[idx..tag_end];
        let href = extract_attr(tag_slice, "href");
        if href.as_deref().is_none_or(|h| h.is_empty() || h == "#") {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ButtonMissingHref,
                message: "`<mj-button>` missing real href (empty or `#`)".into(),
                hint:
                    "Every button must have a real target URL or a merge tag like `{{ cta_url }}`."
                        .into(),
            });
            break;
        }
    }

    // 10. Triple-brace allowlist
    for captured in extract_triple_brace_names(&body) {
        if !ALLOWED_TRIPLE_BRACE.contains(&captured.as_str()) {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ForbiddenTripleBrace,
                message: format!(
                    "`{{{{{{ {captured} }}}}}}` uses triple-brace (raw HTML) but is not in the allowlist"
                ),
                hint: "Triple-brace is reserved for `unsubscribe_link` and `physical_address_footer`. Use double-brace `{{ name }}` for contact fields.".into(),
            });
        }
    }

    // 11. Forbidden helpers: {{#each}}, {{> partial}}
    if body.contains("{{#each") || body.contains("{{> ") {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::ForbiddenHelper,
            message: "template uses `{{#each}}` or `{{> partial}}` which are not supported in v0.1".into(),
            hint: "Phase 4 supports scalar variables + `{{#if}}` / `{{#unless}}` only. Loops and partials land in v0.2+.".into(),
        });
    }

    // 12. Declared-vs-used variable check
    //    - Declared but not used in body or subject → ERROR (Codex FIX #6)
    //    - Used but not declared (and not in built-ins) → ERROR
    for var in &schema.variables {
        let tag2 = format!("{{{{ {} }}}}", var.name);
        let tag3 = format!("{{{{{{ {} }}}}}}", var.name);
        let tag2_sub = schema.subject.contains(&tag2);
        let tag_in_body = body.contains(&tag2) || body.contains(&tag3);
        if !tag2_sub && !tag_in_body {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::UnusedVariable,
                message: format!("variable `{}` is declared but never used", var.name),
                hint: format!(
                    "Either remove `{}` from the frontmatter or reference it in the body/subject",
                    var.name
                ),
            });
        }
    }

    // Scan body + subject for `{{ identifier }}` tokens and verify each is declared or built-in.
    let mut used = extract_merge_tag_names(&body);
    used.extend(extract_merge_tag_names(&schema.subject));
    for captured in used {
        let declared = schema.variables.iter().any(|v| v.name == captured);
        let built_in = BUILT_INS.contains(&captured.as_str());
        if !declared && !built_in {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::UndeclaredVariable,
                message: format!("variable `{captured}` is used but not declared in frontmatter"),
                hint: format!(
                    "Add `- name: {captured}\\n    type: string\\n    required: false` to `variables:` or use one of the built-ins: {}",
                    BUILT_INS.join(", ")
                ),
            });
        }
    }

    // 13. Compile + measure size.
    //    Use `compile_with_placeholders` so the send-time placeholders get stub
    //    values and we can measure the realistic post-inline HTML size.
    let stub_data = stub_data_for_variables(&schema);
    match compile_with_placeholders(source, &stub_data) {
        Ok(Rendered { html, text, .. }) => {
            if html.len() >= GMAIL_CLIP_ERROR {
                findings.push(LintFinding {
                    severity: Severity::Error,
                    rule: LintRule::HtmlSizeError,
                    message: format!(
                        "post-inline HTML is {} bytes (Gmail clips at {} bytes — the footer and unsubscribe link will be hidden)",
                        html.len(), GMAIL_CLIP_ERROR
                    ),
                    hint: "Reduce the template size — inline smaller images, remove redundant sections, or move content to a landing page. A clipped footer is a compliance failure, not just an aesthetic problem.".into(),
                });
            } else if html.len() >= GMAIL_CLIP_WARN {
                findings.push(LintFinding {
                    severity: Severity::Warning,
                    rule: LintRule::HtmlSizeWarning,
                    message: format!(
                        "post-inline HTML is {} bytes (Gmail clips at {} bytes — you're close to the cliff)",
                        html.len(), GMAIL_CLIP_ERROR
                    ),
                    hint: "Consider trimming the template before it grows past the clip limit.".into(),
                });
            }
            if text.trim().is_empty() {
                findings.push(LintFinding {
                    severity: Severity::Error,
                    rule: LintRule::EmptyPlainText,
                    message: "plain-text alternative is empty".into(),
                    hint: "html2text failed to extract readable text — ensure the template has actual <mj-text> content".into(),
                });
            }
        }
        Err(e) => {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::MjmlParseFailed,
                message: format!("compile failed: {e}"),
                hint: "Run `template render <name>` to see the full compile error".into(),
            });
        }
    }

    summarize(findings)
}

/// Find all byte positions where `needle` appears in `haystack` (non-overlapping).
fn find_tag_positions(haystack: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(idx) = haystack[start..].find(needle) {
        out.push(start + idx);
        start += idx + needle.len();
    }
    out
}

/// Extract the value of `attr="value"` from a tag slice. Returns `None` if
/// the attribute is missing or malformed.
fn extract_attr(tag_slice: &str, attr: &str) -> Option<String> {
    let pat = format!("{attr}=\"");
    let idx = tag_slice.find(&pat)? + pat.len();
    let end = tag_slice[idx..].find('"')?;
    Some(tag_slice[idx..idx + end].to_string())
}

/// Extract `{{{ name }}}` identifiers (triple-brace).
fn extract_triple_brace_names(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search_start = 0;
    while let Some(rel) = body[search_start..].find("{{{") {
        let open = search_start + rel + 3;
        if let Some(close_rel) = body[open..].find("}}}") {
            let inner = body[open..open + close_rel].trim();
            if !inner.starts_with('#') && !inner.starts_with('/') {
                let name: String = inner
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    out.push(name);
                }
            }
            search_start = open + close_rel + 3;
        } else {
            break;
        }
    }
    out
}

fn stub_data_for_variables(schema: &crate::template::frontmatter::VarSchema) -> Value {
    let mut map = serde_json::Map::new();
    for var in &schema.variables {
        let v = match var.ty.as_str() {
            "number" => json!(0),
            "bool" => json!(false),
            _ => json!("stub"),
        };
        map.insert(var.name.clone(), v);
    }
    // Built-ins that the compile step expects present in non-strict mode.
    map.insert("first_name".into(), json!("stub"));
    map.insert("last_name".into(), json!("stub"));
    map.insert("email".into(), json!("stub@example.invalid"));
    map.insert("current_year".into(), json!(2026));
    map.insert("broadcast_id".into(), json!(0));
    Value::Object(map)
}

/// Extract `{{ name }}` and `{{{ name }}}` identifiers from the template body.
fn extract_merge_tag_names(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Skip a potential third `{`
            let start = if i + 2 < bytes.len() && bytes[i + 2] == b'{' {
                i + 3
            } else {
                i + 2
            };
            // Find the closing `}}`
            if let Some(close_rel) = body[start..].find("}}") {
                let inner = body[start..start + close_rel].trim();
                // skip helpers like `{{#if ...}}` — they start with #
                if !inner.starts_with('#') && !inner.starts_with('/') && !inner.is_empty() {
                    // An identifier, possibly with leading triple-brace trimmed already
                    let name: String = inner
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.push(name);
                    }
                }
                i = start + close_rel + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn summarize(findings: Vec<LintFinding>) -> LintOutcome {
    let error_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warning_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    LintOutcome {
        findings,
        error_count,
        warning_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"---
name: welcome
subject: "Welcome, {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Welcome</mj-title>
    <mj-preview>Welcome, new friend</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hi {{ first_name }}</mj-text>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

    #[test]
    fn lints_clean_template() {
        let outcome = lint(GOOD);
        assert_eq!(outcome.error_count, 0, "findings: {:?}", outcome.findings);
    }

    #[test]
    fn flags_missing_unsubscribe_link() {
        let src = GOOD.replace("{{{ unsubscribe_link }}}", "");
        let outcome = lint(&src);
        assert!(outcome.has_errors());
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.rule == LintRule::UnsubscribeLinkMissing)
        );
    }

    #[test]
    fn flags_missing_physical_address_footer() {
        let src = GOOD.replace("{{{ physical_address_footer }}}", "");
        let outcome = lint(&src);
        assert!(outcome.has_errors());
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.rule == LintRule::PhysicalAddressFooterMissing)
        );
    }

    #[test]
    fn flags_dangerous_css() {
        let src = GOOD.replace("<mj-text>", "<mj-text css-class=\"display:flex;\">");
        let outcome = lint(&src);
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.rule == LintRule::DangerousCss)
        );
    }

    #[test]
    fn flags_unused_variable() {
        let src = GOOD.replace(
            "variables:\n  - name: first_name",
            "variables:\n  - name: first_name\n    type: string\n    required: false\n  - name: unused_var",
        );
        let outcome = lint(&src);
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.rule == LintRule::UnusedVariable)
        );
    }

    #[test]
    fn flags_missing_mj_preview_as_warning() {
        let src = GOOD.replace("<mj-preview>Welcome, new friend</mj-preview>", "");
        let outcome = lint(&src);
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.rule == LintRule::MjPreviewMissing)
        );
    }

    #[test]
    fn flags_subject_too_long_as_warning() {
        let long_subject = "x".repeat(120);
        let src = GOOD.replace("Welcome, {{ first_name }}", &long_subject);
        let outcome = lint(&src);
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.rule == LintRule::SubjectTooLong)
        );
    }
}
