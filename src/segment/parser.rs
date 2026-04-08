//! Filter expression parser. Wraps pest.
//! Full implementation lands in Task 4.

use crate::segment::ast::SegmentExpr;

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

pub fn parse(_input: &str) -> Result<SegmentExpr, ParseError> {
    Err(ParseError::new(
        "filter parser not yet implemented",
        "implement in Task 4",
    ))
}
