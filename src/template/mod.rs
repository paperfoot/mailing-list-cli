//! Template subsystem (v0.2): plain HTML source + `{{ var }}` substitution.
//!
//! v0.1 used MJML via `mrml`, YAML frontmatter with a variable schema, a
//! Handlebars compile pipeline, `css-inline` for Outlook, `html2text` for
//! plain-text alternatives, and a 20-rule lint. v0.2 drops all of that.
//! Templates are now plain HTML files with a hand-rolled `{{ var }}` +
//! `{{#if}}` substituter and a 6-rule lint that only catches things a
//! browser preview can't (CAN-SPAM placeholders, Gmail 102K clip, XSS
//! allowlist, forbidden tags, send-time unresolved placeholders).
//!
//! Pipeline:
//!
//!   html_source + subject + merge_data
//!     → render::substitute (merge tags, HTML-escape, {{#if}} blocks)
//!     → render::lint (inline, 6 rules)
//!     → render::html_to_text (plain-text alt)
//!     → Rendered { subject, html, text, size_bytes, findings, unresolved }
//!
//! Two entry points:
//!   - `render::render()` — strict (unresolved = error). Called by the send pipeline.
//!   - `render::render_preview()` — lenient (unresolved = warning). Called by
//!     `template preview`, `template render`, and `template lint`.

pub mod render;
pub mod subst;

#[allow(unused_imports)]
pub use render::{
    LintFinding, LintRule, RenderError, Rendered, Severity, lint, render, render_preview,
};
#[allow(unused_imports)]
pub use subst::{SubstResult, substitute};
