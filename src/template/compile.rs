//! Template compile pipeline. Full implementation lands in Task 3.

use crate::template::frontmatter::FrontmatterError;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("frontmatter error: {0}")]
    Frontmatter(#[from] FrontmatterError),
    #[error("handlebars render error: {0}")]
    Handlebars(String),
    #[error("mjml parse error: {0}")]
    Mjml(String),
    #[error("css inline error: {0}")]
    Inline(String),
    #[error("required variable missing: {0}")]
    MissingVariable(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    pub subject: String,
    pub html: String,
    pub text: String,
    pub size_bytes: usize,
}

/// Compile a template source string with the given merge data.
/// Merge-data `{{{ unsubscribe_link }}}` and `{{{ physical_address_footer }}}`
/// are expected to be pre-filled by the caller when non-empty; otherwise they
/// pass through as literal triple-brace tokens and the linter can detect them.
pub fn compile(_source: &str, _data: &Value) -> Result<Rendered, CompileError> {
    // Implemented in Task 3.
    Err(CompileError::Handlebars("not yet implemented".into()))
}

/// Same as `compile`, but substitutes placeholder stub values for the two
/// send-time-only merge tags. Used by `template render --with-placeholders`
/// for preview.
pub fn compile_with_placeholders(_source: &str, _data: &Value) -> Result<Rendered, CompileError> {
    // Implemented in Task 3.
    Err(CompileError::Handlebars("not yet implemented".into()))
}
