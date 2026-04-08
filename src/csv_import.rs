//! Streaming CSV import for `contact import`.
//!
//! Spec §9.3: every row must carry a `consent_source` column, unless the caller
//! passes `--unsafe-no-consent` (which tags every row `imported_without_consent`).
//! Resumability contract: the importer is idempotent under replay — running the
//! same file twice produces no duplicate contacts, tags, or field values.

use crate::db::Db;
use crate::error::AppError;
use std::io::Read;

/// One validated CSV row, ready for DB writes.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportRow {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub consent_source: Option<String>,
    pub tags: Vec<String>,
    pub fields: Vec<(String, String)>, // raw string pairs, coerced at write time
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportSummary {
    pub total_rows: usize,
    pub inserted: usize,
    pub skipped_suppressed: usize,
    pub skipped_invalid: usize,
    pub tagged_without_consent: usize,
    pub errors: Vec<String>,
}

/// Parse a CSV reader into validated rows. Returns a hard error on missing
/// `consent_source` column (unless `unsafe_no_consent` is true) or malformed
/// CSV. Individual row errors accumulate in `ImportSummary.errors` rather
/// than aborting the whole import.
pub fn read_rows<R: Read>(reader: R, unsafe_no_consent: bool) -> Result<Vec<ImportRow>, AppError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(reader);

    let headers = rdr
        .headers()
        .map_err(|e| AppError::BadInput {
            code: "csv_header_error".into(),
            message: format!("could not read CSV headers: {e}"),
            suggestion: "Verify the file starts with a header row like `email,first_name,...`"
                .into(),
        })?
        .clone();

    // Required: email
    let email_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("email"))
        .ok_or_else(|| AppError::BadInput {
            code: "csv_missing_email_column".into(),
            message: "CSV must have an 'email' column".into(),
            suggestion: "Add a column named 'email' to the CSV header row".into(),
        })?;

    let consent_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("consent_source"));

    if consent_idx.is_none() && !unsafe_no_consent {
        return Err(AppError::BadInput {
            code: "csv_missing_consent_source".into(),
            message: "CSV has no 'consent_source' column (required by spec §9.3)".into(),
            suggestion:
                "Either add a 'consent_source' column to the CSV, or rerun with --unsafe-no-consent (imported rows will be auto-tagged `imported_without_consent`)"
                    .into(),
        });
    }

    let first_name_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("first_name"));
    let last_name_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("last_name"));
    let tags_idx = headers.iter().position(|h| h.eq_ignore_ascii_case("tags"));

    // Any column that isn't one of the well-known ones becomes a custom-field entry.
    let well_known: Vec<usize> = [
        Some(email_idx),
        consent_idx,
        first_name_idx,
        last_name_idx,
        tags_idx,
    ]
    .into_iter()
    .flatten()
    .collect();
    let field_indices: Vec<(usize, String)> = headers
        .iter()
        .enumerate()
        .filter(|(i, _)| !well_known.contains(i))
        .map(|(i, h)| (i, h.to_string()))
        .collect();

    let mut rows = Vec::new();
    for (row_num, rec) in rdr.records().enumerate() {
        let rec = match rec {
            Ok(r) => r,
            Err(e) => {
                return Err(AppError::BadInput {
                    code: "csv_parse_error".into(),
                    message: format!("row {}: {e}", row_num + 2),
                    suggestion: "Check for unterminated quotes or mismatched column counts".into(),
                });
            }
        };
        let email = rec.get(email_idx).map(str::to_string).unwrap_or_default();
        if email.is_empty() {
            continue; // skip empty rows silently
        }
        let consent_source = consent_idx.and_then(|i| rec.get(i).map(str::to_string));
        if !unsafe_no_consent
            && consent_source
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(AppError::BadInput {
                code: "csv_row_missing_consent".into(),
                message: format!("row {} for '{email}' has empty consent_source", row_num + 2),
                suggestion: "Populate consent_source for every row, or use --unsafe-no-consent"
                    .into(),
            });
        }
        let first_name = first_name_idx.and_then(|i| rec.get(i).map(str::to_string));
        let last_name = last_name_idx.and_then(|i| rec.get(i).map(str::to_string));
        let tags: Vec<String> = tags_idx
            .and_then(|i| rec.get(i))
            .map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let fields: Vec<(String, String)> = field_indices
            .iter()
            .filter_map(|(i, name)| {
                rec.get(*i)
                    .filter(|v| !v.is_empty())
                    .map(|v| (name.clone(), v.to_string()))
            })
            .collect();

        rows.push(ImportRow {
            email,
            first_name: first_name.filter(|s| !s.is_empty()),
            last_name: last_name.filter(|s| !s.is_empty()),
            consent_source,
            tags,
            fields,
        });
    }
    Ok(rows)
}

/// Apply validated rows to the database. Idempotent — safe to rerun.
/// Does NOT call `email-cli`; that layer is handled by the command module so
/// it can enforce rate limits and progress reporting.
pub fn apply_row_local(
    db: &Db,
    list_id: i64,
    row: &ImportRow,
    unsafe_no_consent: bool,
) -> Result<(), AppError> {
    // Suppression check (idempotent; suppressed rows become no-ops)
    if is_suppressed(db, &row.email)? {
        return Err(AppError::BadInput {
            code: "contact_suppressed".into(),
            message: format!("{} is on the global suppression list", row.email),
            suggestion: "inspect with `mailing-list-cli suppression ls` (Phase 7)".into(),
        });
    }

    let contact_id = db.contact_upsert(
        &row.email,
        row.first_name.as_deref(),
        row.last_name.as_deref(),
    )?;
    db.contact_add_to_list(contact_id, list_id)?;

    // Tags
    let mut resolved_tags: Vec<String> = row.tags.clone();
    if unsafe_no_consent {
        resolved_tags.push("imported_without_consent".to_string());
    }
    for tag in &resolved_tags {
        let tag_id = db.tag_get_or_create(tag)?;
        db.contact_tag_add(contact_id, tag_id)?;
    }

    // Fields (type-coerced; unknown keys are a hard error so the operator
    // notices bad CSV columns early)
    for (k, v) in &row.fields {
        let field = db.field_get(k)?.ok_or_else(|| AppError::BadInput {
            code: "field_not_found".into(),
            message: format!(
                "CSV has column '{k}' but no matching field exists; create it with `field create {k} --type text`"
            ),
            suggestion: "Either remove the column or run `field create` first".into(),
        })?;
        let typed = db.coerce_field_value(&field, v)?;
        db.contact_field_upsert(contact_id, field.id, &typed)?;
    }
    Ok(())
}

fn is_suppressed(db: &Db, email: &str) -> Result<bool, AppError> {
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM suppression WHERE email = ?1",
            rusqlite::params![email],
            |r| r.get(0),
        )
        .map_err(|e| AppError::Transient {
            code: "suppression_lookup_failed".into(),
            message: format!("could not query suppression: {e}"),
            suggestion: "Run `mailing-list-cli health`".into(),
        })?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CSV: &str = "\
email,first_name,last_name,tags,consent_source,company
alice@example.com,Alice,Smith,vip;early,landing-page,Acme
bob@example.com,Bob,,,manual,Globex
";

    #[test]
    fn reads_headers_and_rows() {
        let rows = read_rows(SAMPLE_CSV.as_bytes(), false).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].email, "alice@example.com");
        assert_eq!(rows[0].first_name.as_deref(), Some("Alice"));
        assert_eq!(rows[0].consent_source.as_deref(), Some("landing-page"));
        assert_eq!(
            rows[0].fields,
            vec![("company".to_string(), "Acme".to_string())]
        );
        // ';' is NOT a separator — only ',' — so tags end up as one string
        assert_eq!(rows[0].tags, vec!["vip;early".to_string()]);
    }

    #[test]
    fn rejects_missing_consent_source_column() {
        let csv = "email,first_name\nalice@example.com,Alice\n";
        let err = read_rows(csv.as_bytes(), false).unwrap_err();
        assert_eq!(err.code(), "csv_missing_consent_source");
    }

    #[test]
    fn accepts_missing_consent_with_unsafe_flag() {
        let csv = "email,first_name\nalice@example.com,Alice\n";
        let rows = read_rows(csv.as_bytes(), true).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn rejects_empty_consent_value_without_unsafe() {
        let csv = "email,consent_source\nalice@example.com,\n";
        let err = read_rows(csv.as_bytes(), false).unwrap_err();
        assert_eq!(err.code(), "csv_row_missing_consent");
    }

    #[test]
    fn comma_separated_tags_split_correctly() {
        let csv = "email,tags,consent_source\nalice@example.com,\"vip,early\",manual\n";
        let rows = read_rows(csv.as_bytes(), false).unwrap();
        assert_eq!(rows[0].tags, vec!["vip".to_string(), "early".to_string()]);
    }

    #[test]
    fn apply_row_is_idempotent() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();
        let row = ImportRow {
            email: "alice@example.com".into(),
            first_name: Some("Alice".into()),
            last_name: None,
            consent_source: Some("manual".into()),
            tags: vec!["vip".into()],
            fields: vec![],
        };
        apply_row_local(&db, list_id, &row, false).unwrap();
        apply_row_local(&db, list_id, &row, false).unwrap();
        apply_row_local(&db, list_id, &row, false).unwrap();
        // Exactly one contact, one tag linkage
        let contacts = db.contact_list_in_list(list_id, 100).unwrap();
        assert_eq!(contacts.len(), 1);
        let tag_rows = db.contact_tags_for(contacts[0].id).unwrap();
        assert_eq!(tag_rows, vec!["vip".to_string()]);
    }

    #[test]
    fn apply_row_refuses_suppressed_email() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();
        db.conn
            .execute(
                "INSERT INTO suppression (email, reason, suppressed_at) VALUES ('blocked@example.com', 'hard_bounced', '2026-01-01')",
                [],
            )
            .unwrap();
        let row = ImportRow {
            email: "blocked@example.com".into(),
            first_name: None,
            last_name: None,
            consent_source: Some("manual".into()),
            tags: vec![],
            fields: vec![],
        };
        assert!(apply_row_local(&db, list_id, &row, false).is_err());
    }

    #[test]
    fn apply_row_auto_tags_unsafe_consent() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();
        let row = ImportRow {
            email: "alice@example.com".into(),
            first_name: None,
            last_name: None,
            consent_source: None,
            tags: vec![],
            fields: vec![],
        };
        apply_row_local(&db, list_id, &row, true).unwrap();
        let contact = db
            .contact_get_by_email("alice@example.com")
            .unwrap()
            .unwrap();
        assert!(
            db.contact_tags_for(contact.id)
                .unwrap()
                .contains(&"imported_without_consent".to_string())
        );
    }
}
