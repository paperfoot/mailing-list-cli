//! JSON batch file writer for `email-cli batch send`.
//!
//! Resend's batch send takes up to 100 entries per call. Each entry is a full
//! send request: from, to, subject, html, text, headers, tags.

use crate::error::AppError;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct BatchEntry {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub html: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    pub headers: serde_json::Value,
    pub tags: Vec<serde_json::Value>,
}

#[allow(dead_code)]
pub fn write_batch_file(entries: &[BatchEntry], path: &Path) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| AppError::Transient {
        code: "batch_serialize_failed".into(),
        message: format!("could not serialize batch entries: {e}"),
        suggestion: "Check for non-UTF-8 bytes in template output".into(),
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Transient {
            code: "batch_mkdir_failed".into(),
            message: format!("could not create batch dir {}: {e}", parent.display()),
            suggestion: "Check ~/.cache/mailing-list-cli/ permissions".into(),
        })?;
    }
    std::fs::write(path, json).map_err(|e| AppError::Transient {
        code: "batch_write_failed".into(),
        message: format!("could not write batch file {}: {e}", path.display()),
        suggestion: "Check filesystem write permissions".into(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn writes_batch_file_as_json_array() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let entry = BatchEntry {
            from: "sender@example.com".into(),
            to: vec!["alice@example.com".into()],
            subject: "Hi".into(),
            html: "<p>hello</p>".into(),
            text: "hello".into(),
            reply_to: None,
            headers: json!({"List-Unsubscribe": "<https://x/u/tok>"}),
            tags: vec![json!({"name": "broadcast_id", "value": "1"})],
        };
        write_batch_file(&[entry], tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed[0]["from"], "sender@example.com");
        assert_eq!(parsed[0]["to"][0], "alice@example.com");
    }
}
