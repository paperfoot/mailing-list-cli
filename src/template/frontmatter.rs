//! YAML frontmatter parsing — manual split + serde_yaml.
//!
//! Contract:
//!   - The file MUST start with a line `---` (optionally with trailing \r).
//!   - Everything between that line and the next `---` line (exclusive) is YAML.
//!   - Everything after the closing `---` line is the template body.
//!
//! Deterministic errors with precise codes. Chosen over `gray_matter` for
//! simplicity and one fewer dep.

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum FrontmatterError {
    #[error("frontmatter missing: templates must start with a `---` delimited YAML block")]
    Missing,
    #[error("frontmatter closing `---` not found")]
    UnclosedBlock,
    #[error("frontmatter YAML parse error: {0}")]
    Yaml(String),
    #[error("frontmatter is missing required field: {0}")]
    MissingField(&'static str),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarSchema {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String, // "string" | "number" | "bool"
    #[serde(default)]
    pub required: bool,
}

/// Parsed template = schema + the MJML body with merge tags still in place.
#[derive(Debug, Clone)]
pub struct ParsedTemplate {
    pub schema: VarSchema,
    pub body: String,
}

/// Split a raw template source into its YAML frontmatter schema and the MJML body.
pub fn split_frontmatter(source: &str) -> Result<ParsedTemplate, FrontmatterError> {
    // Find the opening `---` line (first line of the file, possibly trimmed).
    let rest = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))
        .ok_or(FrontmatterError::Missing)?;

    // Find the closing `---` line. We scan for the literal "\n---\n" or "\n---\r\n" or
    // "\n---" followed by EOF.
    let (yaml_block, body) = find_closing(rest).ok_or(FrontmatterError::UnclosedBlock)?;

    let schema: VarSchema =
        serde_yaml::from_str(yaml_block).map_err(|e| FrontmatterError::Yaml(e.to_string()))?;

    if schema.name.is_empty() {
        return Err(FrontmatterError::MissingField("name"));
    }
    if schema.subject.is_empty() {
        return Err(FrontmatterError::MissingField("subject"));
    }

    Ok(ParsedTemplate {
        schema,
        body: body.to_string(),
    })
}

fn find_closing(rest: &str) -> Option<(&str, &str)> {
    // Look for a line that is exactly "---" (followed by \n, \r\n, or EOF).
    let mut idx = 0;
    while idx < rest.len() {
        let slice = &rest[idx..];
        if slice.starts_with("---\n") {
            return Some((&rest[..idx], &rest[idx + 4..]));
        }
        if slice.starts_with("---\r\n") {
            return Some((&rest[..idx], &rest[idx + 5..]));
        }
        if slice == "---" {
            return Some((&rest[..idx], ""));
        }
        // Advance to the next line start.
        match rest[idx..].find('\n') {
            Some(nl) => idx += nl + 1,
            None => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_TEMPLATE: &str = r#"---
name: welcome
subject: "Welcome, {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml><mj-body><mj-section><mj-column>
  <mj-text>Hi {{ first_name }}</mj-text>
  {{{ unsubscribe_link }}}
  {{{ physical_address_footer }}}
</mj-column></mj-section></mj-body></mjml>
"#;

    #[test]
    fn splits_valid_template() {
        let parsed = split_frontmatter(MINIMAL_TEMPLATE).unwrap();
        assert_eq!(parsed.schema.name, "welcome");
        assert_eq!(parsed.schema.subject, "Welcome, {{ first_name }}");
        assert_eq!(parsed.schema.variables.len(), 1);
        assert_eq!(parsed.schema.variables[0].name, "first_name");
        assert!(parsed.schema.variables[0].required);
        assert!(parsed.body.contains("<mjml>"));
        assert!(parsed.body.contains("{{{ unsubscribe_link }}}"));
    }

    #[test]
    fn rejects_template_without_frontmatter() {
        let result = split_frontmatter("<mjml></mjml>");
        assert!(matches!(result, Err(FrontmatterError::Missing)));
    }

    #[test]
    fn rejects_missing_name() {
        let src = "---\nsubject: hi\n---\n<mjml></mjml>";
        let err = split_frontmatter(src).unwrap_err();
        match err {
            FrontmatterError::MissingField("name") => {}
            _ => panic!("expected MissingField(name), got {err:?}"),
        }
    }

    #[test]
    fn rejects_missing_subject() {
        let src = "---\nname: foo\n---\n<mjml></mjml>";
        let err = split_frontmatter(src).unwrap_err();
        match err {
            FrontmatterError::MissingField("subject") => {}
            _ => panic!("expected MissingField(subject), got {err:?}"),
        }
    }
}
