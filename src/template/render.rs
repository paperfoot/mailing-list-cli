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
    // Strip HTML comments BEFORE substitution and lint scanning. Comments are
    // author-facing documentation (the built-in scaffold uses one as a
    // quick-reference), not content for the recipient. Leaving them in would:
    //   - let literal `<script>` / `<iframe>` text inside a comment trip the
    //     `forbidden_tag` lint rule
    //   - let literal `{{{ var }}}` text inside a comment trip the
    //     `forbidden_raw_injection` lint rule
    //   - ship author comments to subscribers (mild bandwidth waste and a bit
    //     of information leakage)
    // The raw source in the DB is unchanged — `template show` still prints
    // the comment so agents can read it.
    let html_source = strip_html_comments(html_source);

    // Subject goes through substitution too — agents put `{{ first_name }}`
    // in subjects all the time.
    let subject_sub = substitute(subject_source, data);
    let body_sub = substitute(&html_source, data);

    // v0.4 (MON-1): inject UTM params on every outbound link in the
    // substituted HTML. Runs AFTER substitution so {{ landing_page_url }}
    // merge tags get UTM params on the resolved URL, not on the tag syntax.
    // Runs BEFORE lint + html_to_text so the decorated links appear in both
    // the HTML and the plain-text fallback.
    let body_html = inject_utm_params(&body_sub.output, data);

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
    let html_size = body_html.len();
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

    let text = html_to_text(&body_html);

    Rendered {
        subject: subject_sub.output,
        html: body_html,
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

    // Unescape HTML entities. We cover:
    //   - the five XML-safe primitives (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`)
    //   - `&nbsp;` → ' '
    //   - common typographic named entities agents are likely to reach for
    //     (em-dash, en-dash, curly quotes, ellipsis, bullet, middot, copyright,
    //     registered, trademark, degree, plus-minus, multiply, divide)
    //   - decimal numeric character references `&#NNN;`
    //   - hexadecimal numeric character references `&#xHH;` (case-insensitive)
    //
    // `&amp;` is unescaped LAST so named entities containing a literal `&amp;`
    // (which should never happen in compliant HTML but might appear in wild
    // copy) aren't double-unescaped into garbage.
    let unescaped = unescape_entities(&out);

    // Collapse consecutive whitespace while preserving double-newlines.
    collapse_whitespace(&unescaped)
}

/// Strip HTML comments (`<!-- ... -->`) from the source. Comments nest
/// illegally in HTML so we use a flat non-greedy scan. Multi-line comments
/// are fine; unterminated comments consume to EOF (consistent with how most
/// HTML parsers recover from malformed input).
///
/// Called by `render_inner` before substitution + lint scan. The raw source
/// in the database is unchanged; this is a render-time transform only.
fn strip_html_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let bytes = source.as_bytes();
    while i < bytes.len() {
        if source[i..].starts_with("<!--") {
            if let Some(close) = source[i + 4..].find("-->") {
                i += 4 + close + 3; // skip past `-->`
                continue;
            }
            // Unterminated comment → drop everything from here to EOF.
            break;
        }
        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// v0.4 (MON-1): inject UTM query parameters on every `<a href="...">` link
/// in the already-substituted HTML. Skips:
///   - excluded schemes: `mailto:`, `tel:`, `sms:`, `javascript:`, `data:`
///   - links with a `data-utm="off"` attribute on the `<a>` tag
///   - links that are fragments only (`#section`)
///
/// Handles both `href="..."` (double-quote) and `href='...'` (single-quote).
/// Correctly appends `?` vs `&` depending on whether the URL already has a
/// query string. Preserves `#fragment` by inserting params BEFORE the hash.
///
/// UTM values come from the merge data if present, otherwise defaults:
///   - `utm_source` = "mailing-list-cli"
///   - `utm_medium` = "email"
///   - `utm_campaign` = merge data `broadcast_name` or "broadcast"
///
/// The `data` parameter is the same merge data passed to `substitute`; it's
/// used here only to extract `broadcast_name` for the campaign tag.
fn inject_utm_params(html: &str, data: &serde_json::Value) -> String {
    let campaign = data
        .get("broadcast_name")
        .and_then(|v| v.as_str())
        .unwrap_or("broadcast");
    let utm = format!(
        "utm_source=mailing-list-cli&utm_medium=email&utm_campaign={}",
        percent_encode_simple(campaign)
    );

    let mut out = String::with_capacity(html.len() + html.len() / 10);
    let mut i = 0;
    let lower = html.to_ascii_lowercase();

    while i < html.len() {
        // Look for `<a ` (case-insensitive).
        if lower[i..].starts_with("<a ")
            || lower[i..].starts_with("<a\t")
            || lower[i..].starts_with("<a\n")
        {
            // Find the closing `>` of this tag.
            let tag_end = match html[i..].find('>') {
                Some(pos) => i + pos + 1,
                None => {
                    out.push_str(&html[i..]);
                    break;
                }
            };
            let tag = &html[i..tag_end];
            let tag_lower = &lower[i..tag_end];

            // Check for data-utm="off" → skip rewriting.
            if tag_lower.contains("data-utm=\"off\"") || tag_lower.contains("data-utm='off'") {
                out.push_str(tag);
                i = tag_end;
                continue;
            }

            // Extract href value.
            if let Some(href_start) = tag_lower.find("href=") {
                let after_eq = href_start + 5; // past "href="
                let quote = tag.as_bytes().get(after_eq).copied();
                if quote == Some(b'"') || quote == Some(b'\'') {
                    let q = quote.unwrap() as char;
                    let url_start = after_eq + 1;
                    if let Some(url_len) = tag[url_start..].find(q) {
                        let url = &tag[url_start..url_start + url_len];

                        // Skip excluded schemes.
                        let url_lower = url.to_ascii_lowercase();
                        let excluded = url_lower.starts_with("mailto:")
                            || url_lower.starts_with("tel:")
                            || url_lower.starts_with("sms:")
                            || url_lower.starts_with("javascript:")
                            || url_lower.starts_with("data:")
                            || url.starts_with('#');

                        if excluded {
                            out.push_str(tag);
                        } else {
                            // Insert UTM before fragment, after existing query.
                            let (base, fragment) = match url.find('#') {
                                Some(pos) => (&url[..pos], &url[pos..]),
                                None => (url, ""),
                            };
                            let sep = if base.contains('?') { "&" } else { "?" };
                            let new_url = format!("{base}{sep}{utm}{fragment}");
                            out.push_str(&tag[..url_start]);
                            out.push_str(&new_url);
                            out.push_str(&tag[url_start + url_len..]);
                        }
                        i = tag_end;
                        continue;
                    }
                }
            }
            // No href found or couldn't parse — emit tag unchanged.
            out.push_str(tag);
            i = tag_end;
        } else {
            let ch = html[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Minimal percent-encoding for UTM values: spaces → %20, & → %26, = → %3D.
/// We only need to encode characters that would break a query string.
fn percent_encode_simple(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '&' => out.push_str("%26"),
            '=' => out.push_str("%3D"),
            '#' => out.push_str("%23"),
            '+' => out.push_str("%2B"),
            _ => out.push(c),
        }
    }
    out
}

/// Decode common HTML entities into their literal characters for the
/// plain-text MIME alternative. We handle:
///   - the five XML-safe primitives plus `&nbsp;`
///   - a curated set of typographic named entities that product copy is
///     likely to use (em-dash, curly quotes, ellipsis, copyright, etc.)
///   - decimal numeric character references `&#NNN;`
///   - hexadecimal numeric character references `&#xHH;`
///
/// Anything unknown is left literal. We deliberately do not include a huge
/// HTML5 entity table — the goal is "text version of marketing copy" not
/// "general-purpose HTML parser".
fn unescape_entities(s: &str) -> String {
    // Two passes. First pass: numeric character references via a manual
    // scanner (so we can handle `&#NNN;` and `&#xHH;` without regex). Second
    // pass: a fixed list of named entities via `str::replace` (simpler and
    // sufficient for a short list). Named entities are matched by longest
    // first to avoid partial matches like `&amp;` being hit by `&amp` first.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' && i + 1 < bytes.len() && bytes[i + 1] == b'#' {
            // Numeric reference. Look for the terminating semicolon within
            // a reasonable distance (max 8 digits).
            let start = i + 2;
            let is_hex = start < bytes.len() && (bytes[start] == b'x' || bytes[start] == b'X');
            let num_start = if is_hex { start + 1 } else { start };
            let mut end = num_start;
            while end < bytes.len() && end < num_start + 8 && bytes[end] != b';' {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b';' && end > num_start {
                let digits = &s[num_start..end];
                let parsed = if is_hex {
                    u32::from_str_radix(digits, 16).ok()
                } else {
                    digits.parse::<u32>().ok()
                };
                if let Some(code) = parsed {
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(s[i..].chars().next().unwrap());
        i += s[i..].chars().next().unwrap().len_utf8();
    }

    // Named entities. Order matters when one is a prefix of another
    // (`&nbsp;` vs `&nb`), but since we only match exact semicolon-terminated
    // sequences, `str::replace` is safe.
    out = out
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&bull;", "•")
        .replace("&middot;", "·")
        .replace("&lsquo;", "‘")
        .replace("&rsquo;", "’")
        .replace("&ldquo;", "“")
        .replace("&rdquo;", "”")
        .replace("&laquo;", "«")
        .replace("&raquo;", "»")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
        .replace("&trade;", "™")
        .replace("&deg;", "°")
        .replace("&plusmn;", "±")
        .replace("&times;", "×")
        .replace("&divide;", "÷")
        .replace("&infin;", "∞")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        // `&amp;` last so decoded entities don't re-enter the table.
        .replace("&amp;", "&");
    out
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
        "physical_address_footer": "<span style=\"color:#666;font-size:11px\">Your Company Name · 123 Example Street · Suite 400 · City, ST 00000 · United States</span>"
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

    #[test]
    fn unescape_named_typographic_entities() {
        // Named entities that product copy is likely to reach for.
        let html = r#"<p>Morning &mdash; here&rsquo;s your order &copy; 2026. Use code &ldquo;WELCOME&rdquo; &bull; valid for 7 days &hellip;</p>"#;
        let text = html_to_text(html);
        assert!(text.contains("—"), "em-dash should decode, got: {text}");
        assert!(text.contains("’"), "right single quote should decode");
        assert!(text.contains("©"), "copyright should decode");
        assert!(text.contains("“"), "left double quote should decode");
        assert!(text.contains("”"), "right double quote should decode");
        assert!(text.contains("•"), "bullet should decode");
        assert!(text.contains("…"), "ellipsis should decode");
        assert!(
            !text.contains("&mdash;") && !text.contains("&copy;"),
            "literal entity text must not survive decoding"
        );
    }

    #[test]
    fn unescape_numeric_character_references() {
        // Decimal and hex numeric refs must decode too.
        let html = r#"<p>Decimal: &#8212; &#169; &#8226;. Hex: &#x2014; &#xA9; &#x2022;.</p>"#;
        let text = html_to_text(html);
        // Both em-dash and copyright should appear twice (once from each form).
        assert_eq!(text.matches('—').count(), 2, "got: {text}");
        assert_eq!(text.matches('©').count(), 2, "got: {text}");
        assert_eq!(text.matches('•').count(), 2, "got: {text}");
        assert!(
            !text.contains("&#") && !text.contains("&#x"),
            "literal numeric ref text must not survive decoding"
        );
    }

    #[test]
    fn unescape_mixed_entities_and_primitives() {
        // Legacy primitives still work alongside the new entries.
        let html = r#"<p>Q&amp;A: Is 5 &lt; 10 &amp; 15 &gt; 10? &mdash; yes.</p>"#;
        let text = html_to_text(html);
        assert!(text.contains("Q&A"));
        assert!(text.contains("5 < 10 & 15 > 10"));
        assert!(text.contains("—"));
    }

    #[test]
    fn html_comments_are_stripped_before_lint_and_substitution() {
        // Comment contains literal `<script>`, `{{{ foo }}}`, and a `{{ bar }}`
        // merge tag — none should trip lint because comments are author docs.
        let source = r##"<!--
          Author notes:
          - don't use <script> or <iframe>
          - don't use {{{ raw }}} outside the allowlist
          - sample merge tag: {{ bar }}
        -->
<p>Real body: {{ first_name }}</p>
<p>{{{ unsubscribe_link }}}</p>
<p>{{{ physical_address_footer }}}</p>"##;
        let r = render_preview(source, "s", &json!({"first_name": "Alice"}));
        assert_eq!(
            r.findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .count(),
            0,
            "comment content should not trigger lint errors, got: {:?}",
            r.findings
        );
        // And the rendered HTML should not contain the comment itself.
        assert!(!r.html.contains("Author notes"), "got: {}", r.html);
        assert!(!r.html.contains("<script"));
        // And the substituter should not have processed `{{ bar }}` inside
        // the comment (so it shouldn't be in unresolved either).
        assert!(
            !r.unresolved.iter().any(|u| u == "bar"),
            "got unresolved: {:?}",
            r.unresolved
        );
        // Real body is still rendered.
        assert!(r.html.contains("Real body: Alice"));
    }

    #[test]
    fn unterminated_comment_drops_to_eof() {
        let source = r##"<p>before</p><!-- never closed <p>{{{ unsubscribe_link }}}</p><p>{{{ physical_address_footer }}}</p>"##;
        let r = render_preview(source, "s", &json!({}));
        // The unterminated comment swallows the rest of the template including
        // the two required placeholders, so lint should fail on both.
        assert!(
            r.findings
                .iter()
                .any(|f| f.rule == LintRule::UnsubscribeLinkMissing),
            "got: {:?}",
            r.findings
        );
        assert!(
            r.findings
                .iter()
                .any(|f| f.rule == LintRule::PhysicalAddressFooterMissing),
            "got: {:?}",
            r.findings
        );
    }

    #[test]
    fn unescape_leaves_unknown_entities_literal() {
        // Unknown entities pass through untouched (better a literal &foo; than
        // a silent wrong character).
        let html = r#"<p>Mystery: &nonexistent; and &maybe; and &1invalid;</p>"#;
        let text = html_to_text(html);
        assert!(text.contains("&nonexistent;"), "got: {text}");
        assert!(text.contains("&maybe;"));
        assert!(text.contains("&1invalid;"));
    }

    // ─── v0.4 MON-1: UTM link injection ─────────────────────────────────

    #[test]
    fn utm_injects_on_simple_link() {
        let html = r#"<a href="https://example.com">Click</a>"#;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert!(
            result.contains("utm_source=mailing-list-cli"),
            "got: {result}"
        );
        assert!(result.contains("utm_medium=email"), "got: {result}");
        assert!(
            result.contains("?utm_source"),
            "first param should use ? not &: {result}"
        );
    }

    #[test]
    fn utm_appends_with_ampersand_when_query_exists() {
        let html = r#"<a href="https://example.com?page=1">Click</a>"#;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert!(
            result.contains("?page=1&utm_source"),
            "should append with & when ? exists: {result}"
        );
    }

    #[test]
    fn utm_preserves_fragment() {
        let html = r#"<a href="https://example.com#section">Click</a>"#;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert!(
            result.contains("utm_campaign=broadcast#section"),
            "fragment should be preserved after UTM params: {result}"
        );
    }

    #[test]
    fn utm_skips_mailto() {
        let html = r#"<a href="mailto:test@example.com">Email</a>"#;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert!(
            !result.contains("utm_source"),
            "mailto links should not get UTM params: {result}"
        );
    }

    #[test]
    fn utm_skips_data_utm_off() {
        let html = r#"<a href="https://example.com" data-utm="off">Click</a>"#;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert!(
            !result.contains("utm_source"),
            "data-utm=off links should not get UTM params: {result}"
        );
    }

    #[test]
    fn utm_uses_broadcast_name_as_campaign() {
        let html = r#"<a href="https://example.com">Click</a>"#;
        let data = serde_json::json!({"broadcast_name": "q1_launch"});
        let result = inject_utm_params(html, &data);
        assert!(
            result.contains("utm_campaign=q1_launch"),
            "campaign should use broadcast_name: {result}"
        );
    }

    #[test]
    fn utm_handles_single_quoted_href() {
        let html = r#"<a href='https://example.com'>Click</a>"#;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert!(
            result.contains("utm_source=mailing-list-cli"),
            "single-quoted href should work: {result}"
        );
    }

    #[test]
    fn utm_skips_fragment_only_link() {
        let html = r##"<a href="#top">Top</a>"##;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert!(
            !result.contains("utm_source"),
            "fragment-only links should not get UTM params: {result}"
        );
    }

    #[test]
    fn utm_leaves_non_link_content_unchanged() {
        let html = r#"<p>Hello world</p><img src="https://example.com/img.png">"#;
        let result = inject_utm_params(html, &serde_json::json!({}));
        assert_eq!(result, html, "non-link content should be untouched");
    }
}
