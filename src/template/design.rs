//! Design-rule checks for email templates (v0.4.5).
//!
//! Distinct from `render::lint`: lint rules cover compliance/security primitives
//! that block sends regardless of how a template is authored (CAN-SPAM
//! placeholders, Gmail clip, forbidden tags, XSS allowlist). Design rules
//! cover authoring shape — "this looks like a browser prototype, not an
//! email" — and historically ran only inside `template inspect` for advisory
//! output. v0.4.5 wires them into `template create` and `broadcast send`
//! preflight so a JSX/React handoff cannot silently flow through the send
//! pipeline.
//!
//! The two error-level codes (`browser_or_jsx_source`,
//! `browser_script_dependency`) indicate "this will not render as email".
//! Warning-level codes are heuristic and surface authoring smells without
//! blocking sends.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesignFinding {
    pub severity: DesignSeverity,
    pub code: &'static str,
    pub message: String,
    pub hint: String,
}

/// Run the design-rule scan against a template source.
///
/// `source_label` is the origin tag (e.g. `template:welcome`, `file:path.jsx`,
/// or `created_template`) and is used both for filename-based JSX detection
/// and as audit metadata.
///
/// Strips HTML comments before scanning. Author comments often contain literal
/// `<script>`/`<iframe>` strings used to *document* what's forbidden (the
/// built-in scaffold does this), and matching on the raw source would
/// false-flag the documentation as a design error. The lint pipeline strips
/// comments for the same reason.
pub fn design_findings(html_source: &str, source_label: &str) -> Vec<DesignFinding> {
    let stripped = super::render::strip_html_comments(html_source);
    let lower = stripped.to_ascii_lowercase();
    let label_lower = source_label.to_ascii_lowercase();
    let mut findings = Vec::new();

    let mut push = |severity: DesignSeverity, code: &'static str, message: &str, hint: &str| {
        findings.push(DesignFinding {
            severity,
            code,
            message: message.to_string(),
            hint: hint.to_string(),
        });
    };

    // v0.4.5: tighter heuristic. The v0.4.4 detector caught explicit
    // `import React` and `.jsx`/`.tsx` paths but missed modern JSX without
    // an explicit React import (Vite/Next 13+) and `export default
    // function`-style components. We also detect a `<Capitalized` tag,
    // which in HTML is invalid but is the dominant component-render pattern
    // in JSX.
    let looks_like_js_source = label_lower.ends_with(".jsx")
        || label_lower.ends_with(".tsx")
        || label_lower.ends_with(".js")
        || label_lower.ends_with(".ts")
        || lower.contains("import react")
        || lower.contains("from 'react'")
        || lower.contains("from \"react\"")
        || lower.contains("reactdom")
        || lower.contains("createroot(")
        || lower.contains("export default function")
        || lower.contains("jsx.element")
        || contains_jsx_component_tag(&stripped);

    if looks_like_js_source {
        push(
            DesignSeverity::Error,
            "browser_or_jsx_source",
            "source looks like a browser/React/JSX prototype, not send-ready email HTML",
            "Extract the visual/content direction, then rewrite it as standalone table-based HTML with inline styles before `template create`.",
        );
    }

    if lower.contains("<script")
        || lower.contains("type=\"text/babel")
        || lower.contains("type='text/babel")
    {
        push(
            DesignSeverity::Error,
            "browser_script_dependency",
            "source depends on JavaScript or Babel",
            "Email clients strip scripts. Remove JS entirely and express the final layout as static HTML tables with inline styles.",
        );
    }

    if lower.contains("<link") && lower.contains("stylesheet") {
        push(
            DesignSeverity::Warning,
            "external_stylesheet",
            "source references an external stylesheet",
            "Inline the required styles onto the elements/cells that need them; most email clients strip or rewrite external CSS.",
        );
    }

    if lower.contains("<style") || lower.contains("@import") {
        push(
            DesignSeverity::Warning,
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
        push(
            DesignSeverity::Warning,
            "browser_layout_css",
            "source uses browser layout CSS such as flex or grid",
            "Rebuild complex rows/columns with presentation tables and inline cell padding for email-client compatibility.",
        );
    }

    if stripped.len() > 1_500 && !lower.contains("<table") {
        push(
            DesignSeverity::Warning,
            "no_table_layout",
            "rich template has no table-based layout",
            "For designed newsletters, use a 100% outer presentation table and a centered 600-640px inner table.",
        );
    }

    if lower.contains("class=") && !lower.contains("style=") {
        push(
            DesignSeverity::Warning,
            "class_styles_without_inline_styles",
            "source appears class-driven rather than inline-styled",
            "Classes alone are fragile in email. Keep important visual styles inline even if classes remain for tooling.",
        );
    }

    findings
}

/// Detect a `<Capitalized` JSX-style component tag in the source. Real HTML
/// elements are lowercase by convention and uppercase tags are passed through
/// the parser as-is, so a `<Header`, `<Card`, `<EmailLayout` opening tag is a
/// strong signal the source is a JSX render tree, not an email body.
fn contains_jsx_component_tag(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' {
            let next = bytes[i + 1];
            if next.is_ascii_uppercase() {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub fn count_severity(findings: &[DesignFinding], target: DesignSeverity) -> usize {
    findings.iter().filter(|f| f.severity == target).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsx_react_import_is_error() {
        let f = design_findings(
            r#"import React from 'react'; export function E() { return <div/>; }"#,
            "file:e.jsx",
        );
        assert!(
            f.iter()
                .any(|x| x.code == "browser_or_jsx_source" && x.severity == DesignSeverity::Error),
            "{f:?}"
        );
    }

    #[test]
    fn modern_jsx_without_react_import_still_caught() {
        // Next 13+ / Vite drops the explicit React import. Component tag is the
        // remaining tell.
        let f = design_findings(
            r#"export default function Page() { return <Header><h1>Hi</h1></Header>; }"#,
            "file:page.tsx",
        );
        assert!(
            f.iter()
                .any(|x| x.code == "browser_or_jsx_source" && x.severity == DesignSeverity::Error)
        );
    }

    #[test]
    fn plain_jsx_extension_alone_is_enough() {
        let f = design_findings("<p>plain text</p>", "file:thing.jsx");
        assert!(f.iter().any(|x| x.code == "browser_or_jsx_source"));
    }

    #[test]
    fn script_tag_is_error() {
        let f = design_findings("<p>x</p><script>evil()</script>", "template:t");
        assert!(
            f.iter()
                .any(|x| x.code == "browser_script_dependency"
                    && x.severity == DesignSeverity::Error)
        );
    }

    #[test]
    fn babel_type_attribute_is_error() {
        let f = design_findings(r#"<p>x</p><span type="text/babel">"#, "template:t");
        assert!(
            f.iter()
                .any(|x| x.code == "browser_script_dependency"
                    && x.severity == DesignSeverity::Error)
        );
    }

    #[test]
    fn external_stylesheet_is_warning() {
        let f = design_findings(
            r#"<link rel="stylesheet" href="x.css"><p>x</p>"#,
            "template:t",
        );
        assert!(
            f.iter()
                .any(|x| x.code == "external_stylesheet" && x.severity == DesignSeverity::Warning)
        );
    }

    #[test]
    fn flex_layout_is_warning() {
        let f = design_findings(r#"<div style="display:flex">x</div>"#, "template:t");
        assert!(
            f.iter()
                .any(|x| x.code == "browser_layout_css" && x.severity == DesignSeverity::Warning)
        );
    }

    #[test]
    fn small_clean_template_has_no_findings() {
        // Below the 1.5KB threshold for the table-layout warning, no JSX, no
        // script, no flex.
        let f = design_findings(
            r#"<!doctype html><body><p>Hi</p></body>"#,
            "template:welcome",
        );
        assert!(
            f.is_empty(),
            "small clean template should have zero findings, got: {f:?}"
        );
    }

    #[test]
    fn count_helpers() {
        let f = design_findings(
            r#"<script>x</script><div style="display:flex">y</div>"#,
            "file:bad.jsx",
        );
        assert!(count_severity(&f, DesignSeverity::Error) >= 2);
        assert!(count_severity(&f, DesignSeverity::Warning) >= 1);
    }

    #[test]
    fn class_without_style_is_warning() {
        let f = design_findings(
            r#"<table class="container"><tr><td class="cell">Hi</td></tr></table>"#,
            "template:t",
        );
        assert!(
            f.iter()
                .any(|x| x.code == "class_styles_without_inline_styles")
        );
    }
}
