//! Template lint rule set. Full implementation lands in Task 4.

use serde::Serialize;

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
    MjmlParseFailed,
    UndeclaredVariable,
    UnusedVariable,
    DangerousCss,
    HtmlSizeTooLarge,
    EmptyPlainText,
    SubjectTooLong,
    SubjectEmpty,
    UnsubscribeLinkMissing,
    PhysicalAddressFooterMissing,
    MjPreviewMissing,
    RawTableOutsideMjRaw,
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

pub fn lint(_source: &str) -> LintOutcome {
    // Implemented in Task 4.
    LintOutcome {
        findings: vec![],
        error_count: 0,
        warning_count: 0,
    }
}
