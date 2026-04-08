//! Filter expression parser. Built on pest.

use crate::segment::ast::SegmentExpr;
use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "segment/grammar.pest"]
struct FilterParser;

#[derive(Debug, thiserror::Error)]
#[error("filter parse error: {message}")]
pub struct ParseError {
    pub message: String,
    pub suggestion: String,
}

impl ParseError {
    pub fn new(message: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            suggestion: suggestion.into(),
        }
    }
}

pub fn parse(input: &str) -> Result<SegmentExpr, ParseError> {
    let mut pairs = FilterParser::parse(Rule::expression, input).map_err(|e| {
        ParseError::new(
            format!("invalid filter expression: {e}"),
            "Check the grammar in `template guidelines` (Phase 4) or the spec §6".to_string(),
        )
    })?;

    // Task 4 replaces this body with full AST construction.
    let _ = pairs.next();
    Err(ParseError::new(
        "AST construction not yet implemented",
        "Task 4 implements the walker",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_accepts_simple_tag() {
        // Just verify pest itself accepts the input; AST construction comes later.
        assert!(FilterParser::parse(Rule::expression, "tag:vip").is_ok());
    }

    #[test]
    fn grammar_accepts_complex_expression() {
        assert!(
            FilterParser::parse(
                Rule::expression,
                "has_tag:premium AND (clicked_last:7d OR opened_last:14d)"
            )
            .is_ok()
        );
    }

    #[test]
    fn grammar_rejects_unclosed_paren() {
        assert!(FilterParser::parse(Rule::expression, "(tag:vip").is_err());
    }
}
