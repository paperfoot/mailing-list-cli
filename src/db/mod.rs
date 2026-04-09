pub mod migrations;

use crate::error::AppError;
use crate::models::{Contact, List};
use crate::paths;
use rusqlite::{Connection, params};
use std::path::Path;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    /// Open the default database. Creates parent directories if needed and runs migrations.
    pub fn open() -> Result<Self, AppError> {
        Self::open_at(&paths::db_path())
    }

    pub fn open_at(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Config {
                code: "db_dir_create_failed".into(),
                message: format!("could not create {}: {e}", parent.display()),
                suggestion: format!("Check directory permissions on {}", parent.display()),
            })?;
        }
        let conn = Connection::open(path).map_err(|e| AppError::Transient {
            code: "db_open_failed".into(),
            message: format!("could not open {}: {e}", path.display()),
            suggestion: "Try removing the file and rerunning to recreate".into(),
        })?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(|e| AppError::Transient {
                code: "db_pragma_failed".into(),
                message: format!("could not set PRAGMAs: {e}"),
                suggestion: "Database may be corrupt; consider recreating".into(),
            })?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> Result<(), AppError> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_version (
                    version TEXT PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );",
            )
            .map_err(|e| AppError::Transient {
                code: "schema_version_bootstrap_failed".into(),
                message: format!("could not create schema_version table: {e}"),
                suggestion: "Database may be corrupt; consider recreating".into(),
            })?;

        for (version, sql) in migrations::MIGRATIONS {
            let already: Option<String> = self
                .conn
                .query_row(
                    "SELECT version FROM schema_version WHERE version = ?",
                    [version],
                    |r| r.get(0),
                )
                .ok();
            if already.is_some() {
                continue;
            }
            self.conn
                .execute_batch(sql)
                .map_err(|e| AppError::Transient {
                    code: "migration_failed".into(),
                    message: format!("migration {version} failed: {e}"),
                    suggestion: format!("Inspect migration {version} for syntax errors"),
                })?;
            let now = chrono::Utc::now().to_rfc3339();
            self.conn
                .execute(
                    "INSERT INTO schema_version (version, applied_at) VALUES (?, ?)",
                    [*version, now.as_str()],
                )
                .map_err(|e| AppError::Transient {
                    code: "schema_version_insert_failed".into(),
                    message: format!("could not record migration: {e}"),
                    suggestion: "Database may be in inconsistent state".into(),
                })?;
        }
        Ok(())
    }

    // ─── List operations ───────────────────────────────────────────────

    pub fn list_create(
        &self,
        name: &str,
        description: Option<&str>,
        resend_segment_id: &str,
    ) -> Result<i64, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO list (name, description, resend_segment_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![name, description, resend_segment_id, now],
            )
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("UNIQUE constraint failed") {
                    AppError::BadInput {
                        code: "list_already_exists".into(),
                        message: format!("a list named '{name}' already exists"),
                        suggestion: "Use `mailing-list-cli list ls` to see existing lists, or pick a different name".into(),
                    }
                } else {
                    AppError::Transient {
                        code: "list_insert_failed".into(),
                        message: format!("could not insert list: {e}"),
                        suggestion: "Try again; if the problem persists, run `mailing-list-cli health`".into(),
                    }
                }
            })?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_get_by_name(&self, name: &str) -> Result<Option<List>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT l.id, l.name, l.description, l.resend_segment_id, l.created_at,
                        COALESCE((SELECT COUNT(*) FROM list_membership lm WHERE lm.list_id = l.id), 0) as member_count
                 FROM list l
                 WHERE l.name = ?1",
            )
            .map_err(query_err)?;
        let row = stmt.query_row(params![name], |row| {
            Ok(List {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                resend_segment_id: row.get(3)?,
                created_at: row.get(4)?,
                member_count: row.get(5)?,
            })
        });
        match row {
            Ok(l) => Ok(Some(l)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn list_all(&self) -> Result<Vec<List>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT l.id, l.name, l.description, l.resend_segment_id, l.created_at,
                        COALESCE((SELECT COUNT(*) FROM list_membership lm WHERE lm.list_id = l.id), 0) as member_count
                 FROM list l
                 WHERE l.archived_at IS NULL
                 ORDER BY l.id ASC",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(List {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    resend_segment_id: row.get(3)?,
                    created_at: row.get(4)?,
                    member_count: row.get(5)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    pub fn list_get_by_id(&self, id: i64) -> Result<Option<List>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT l.id, l.name, l.description, l.resend_segment_id, l.created_at,
                        COALESCE((SELECT COUNT(*) FROM list_membership lm WHERE lm.list_id = l.id), 0) as member_count
                 FROM list l
                 WHERE l.id = ?1",
            )
            .map_err(query_err)?;
        let row = stmt.query_row(params![id], |row| {
            Ok(List {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                resend_segment_id: row.get(3)?,
                created_at: row.get(4)?,
                member_count: row.get(5)?,
            })
        });
        match row {
            Ok(l) => Ok(Some(l)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    // ─── Contact operations ────────────────────────────────────────────

    pub fn contact_upsert(
        &self,
        email: &str,
        first_name: Option<&str>,
        last_name: Option<&str>,
    ) -> Result<i64, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        // Try insert first; if it conflicts, fetch the existing id.
        let res = self.conn.execute(
            "INSERT INTO contact (email, first_name, last_name, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?4)",
            params![email, first_name, last_name, now],
        );
        match res {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            Err(e) if e.to_string().contains("UNIQUE constraint failed") => {
                // Already exists — fetch id
                self.conn
                    .query_row(
                        "SELECT id FROM contact WHERE email = ?1",
                        params![email],
                        |r| r.get::<_, i64>(0),
                    )
                    .map_err(query_err)
            }
            Err(e) => Err(query_err(e)),
        }
    }

    /// Upsert a contact together with its consent record. On first insert
    /// the `consent_source` + `consent_at` columns are populated. On a
    /// re-upsert they are only filled when the existing row has NULL —
    /// previously recorded consent is NEVER overwritten.
    ///
    /// The CSV importer uses the transaction-scoped path directly in
    /// `csv_import::apply_row_inside_tx`; this helper is exposed for
    /// single-row callers (the planned `contact add --consent-source`
    /// flag and any ad-hoc tooling that needs the same semantics).
    #[allow(dead_code)]
    pub fn contact_upsert_with_consent(
        &self,
        email: &str,
        first_name: Option<&str>,
        last_name: Option<&str>,
        consent_source: Option<&str>,
        consent_at: Option<&str>,
    ) -> Result<i64, AppError> {
        let id = self.contact_upsert(email, first_name, last_name)?;
        // Only fill consent_source/consent_at when the stored value is NULL
        // and the caller actually provided a non-empty source.
        if let Some(src) = consent_source.map(str::trim).filter(|s| !s.is_empty()) {
            let existing: Option<String> = self
                .conn
                .query_row(
                    "SELECT consent_source FROM contact WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(query_err)?;
            if existing.is_none() {
                let now = chrono::Utc::now().to_rfc3339();
                let ts = consent_at.unwrap_or(&now);
                self.conn
                    .execute(
                        "UPDATE contact SET consent_source = ?1, consent_at = ?2, updated_at = ?3 WHERE id = ?4",
                        params![src, ts, now, id],
                    )
                    .map_err(query_err)?;
            }
        }
        Ok(id)
    }

    /// Return the stored consent source and recording timestamp for a given
    /// contact (by email), if any. Either or both may be `None`.
    pub fn contact_consent_for_email(
        &self,
        email: &str,
    ) -> Result<Option<ContactConsent>, AppError> {
        match self.conn.query_row(
            "SELECT consent_source, consent_at FROM contact WHERE email = ?1 COLLATE NOCASE",
            params![email],
            |r| {
                Ok(ContactConsent {
                    source: r.get::<_, Option<String>>(0)?,
                    at: r.get::<_, Option<String>>(1)?,
                })
            },
        ) {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn contact_add_to_list(&self, contact_id: i64, list_id: i64) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR IGNORE INTO list_membership (list_id, contact_id, joined_at)
                 VALUES (?1, ?2, ?3)",
                params![list_id, contact_id, now],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn contact_find_id(&self, email: &str) -> Result<Option<i64>, AppError> {
        match self.conn.query_row(
            "SELECT id FROM contact WHERE email = ?1 COLLATE NOCASE",
            params![email],
            |r| r.get::<_, i64>(0),
        ) {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    #[allow(dead_code)] // legacy helper, retained for tests; superseded by segment_members
    pub fn contact_list_in_list(
        &self,
        list_id: i64,
        limit: usize,
    ) -> Result<Vec<Contact>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT c.id, c.email, c.first_name, c.last_name, c.status, c.created_at
                 FROM contact c
                 INNER JOIN list_membership lm ON lm.contact_id = c.id
                 WHERE lm.list_id = ?1
                 ORDER BY c.id ASC
                 LIMIT ?2",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map(params![list_id, limit as i64], |row| {
                Ok(Contact {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    first_name: row.get(2)?,
                    last_name: row.get(3)?,
                    status: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    // ─── Tag operations ────────────────────────────────────────────────

    pub fn tag_get_or_create(&self, name: &str) -> Result<i64, AppError> {
        if let Some(id) = self.tag_find(name)? {
            return Ok(id);
        }
        self.conn
            .execute("INSERT INTO tag (name) VALUES (?1)", params![name])
            .map_err(query_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn tag_find(&self, name: &str) -> Result<Option<i64>, AppError> {
        match self
            .conn
            .query_row("SELECT id FROM tag WHERE name = ?1", params![name], |r| {
                r.get::<_, i64>(0)
            }) {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn tag_all(&self) -> Result<Vec<crate::models::Tag>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.id, t.name,
                        COALESCE((SELECT COUNT(*) FROM contact_tag ct WHERE ct.tag_id = t.id), 0)
                 FROM tag t
                 ORDER BY t.name ASC",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    member_count: row.get(2)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    pub fn tag_delete(&self, name: &str) -> Result<bool, AppError> {
        let affected = self
            .conn
            .execute("DELETE FROM tag WHERE name = ?1", params![name])
            .map_err(query_err)?;
        Ok(affected > 0)
    }

    pub fn contact_tag_add(&self, contact_id: i64, tag_id: i64) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR IGNORE INTO contact_tag (contact_id, tag_id, applied_at)
                 VALUES (?1, ?2, ?3)",
                params![contact_id, tag_id, now],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn contact_tag_remove(&self, contact_id: i64, tag_id: i64) -> Result<bool, AppError> {
        let affected = self
            .conn
            .execute(
                "DELETE FROM contact_tag WHERE contact_id = ?1 AND tag_id = ?2",
                params![contact_id, tag_id],
            )
            .map_err(query_err)?;
        Ok(affected > 0)
    }

    pub fn contact_tags_for(&self, contact_id: i64) -> Result<Vec<String>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.name FROM contact_tag ct
                 JOIN tag t ON ct.tag_id = t.id
                 WHERE ct.contact_id = ?1
                 ORDER BY t.name",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map(params![contact_id], |r| r.get::<_, String>(0))
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    // ─── Field operations ──────────────────────────────────────────────

    pub fn field_create(
        &self,
        key: &str,
        ty: &str,
        options: Option<&[String]>,
    ) -> Result<i64, AppError> {
        if !is_snake_case(key) {
            return Err(AppError::BadInput {
                code: "invalid_field_key".into(),
                message: format!("field key '{key}' must be snake_case"),
                suggestion:
                    "Use lowercase letters, digits, and underscores only (e.g. `company_size`)"
                        .into(),
            });
        }
        if !matches!(ty, "text" | "number" | "date" | "bool" | "select") {
            return Err(AppError::BadInput {
                code: "invalid_field_type".into(),
                message: format!("field type '{ty}' is not valid"),
                suggestion: "Use one of: text, number, date, bool, select".into(),
            });
        }
        if ty == "select" && options.map(|o| o.is_empty()).unwrap_or(true) {
            return Err(AppError::BadInput {
                code: "select_requires_options".into(),
                message: "--type select requires --options to be non-empty".into(),
                suggestion: "Rerun with --options \"red,green,blue\"".into(),
            });
        }
        let options_json = options.map(|o| serde_json::to_string(o).unwrap());
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO field (key, type, options_json, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![key, ty, options_json, now],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed") {
                    AppError::BadInput {
                        code: "field_already_exists".into(),
                        message: format!("a field named '{key}' already exists"),
                        suggestion: "Run `mailing-list-cli field ls` to see existing fields".into(),
                    }
                } else {
                    query_err(e)
                }
            })?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn field_all(&self) -> Result<Vec<crate::models::Field>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, key, type, options_json, created_at FROM field ORDER BY key ASC")
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |row| {
                let options_json: Option<String> = row.get(3)?;
                let options = options_json
                    .as_deref()
                    .map(|s| serde_json::from_str::<Vec<String>>(s).unwrap_or_default());
                Ok(crate::models::Field {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    r#type: row.get(2)?,
                    options,
                    created_at: row.get(4)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    /// Return the declared type of a field (`"text" | "number" | "date" | "bool" | "select"`)
    /// for a given key. Used by the segment compiler to pick the correct
    /// storage column when compiling custom-field predicates.
    pub fn field_get_type(&self, key: &str) -> Result<Option<String>, AppError> {
        match self
            .conn
            .query_row("SELECT type FROM field WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            }) {
            Ok(ty) => Ok(Some(ty)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn field_get(&self, key: &str) -> Result<Option<crate::models::Field>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, key, type, options_json, created_at FROM field WHERE key = ?1")
            .map_err(query_err)?;
        let row = stmt.query_row(params![key], |row| {
            let options_json: Option<String> = row.get(3)?;
            let options = options_json
                .as_deref()
                .map(|s| serde_json::from_str::<Vec<String>>(s).unwrap_or_default());
            Ok(crate::models::Field {
                id: row.get(0)?,
                key: row.get(1)?,
                r#type: row.get(2)?,
                options,
                created_at: row.get(4)?,
            })
        });
        match row {
            Ok(f) => Ok(Some(f)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn field_delete(&self, key: &str) -> Result<bool, AppError> {
        let affected = self
            .conn
            .execute("DELETE FROM field WHERE key = ?1", params![key])
            .map_err(query_err)?;
        Ok(affected > 0)
    }

    // ─── Contact field values ──────────────────────────────────────────

    /// Coerce a string input into the correct typed column based on the
    /// field definition. Returns a `TypedFieldValue` or a `BadInput` error
    /// with an agent-friendly message.
    pub fn coerce_field_value(
        &self,
        field: &crate::models::Field,
        raw: &str,
    ) -> Result<TypedFieldValue, AppError> {
        match field.r#type.as_str() {
            "text" => Ok(TypedFieldValue::Text(raw.to_string())),
            "number" => raw
                .parse::<f64>()
                .map(TypedFieldValue::Number)
                .map_err(|_| AppError::BadInput {
                    code: "field_coercion_failed".into(),
                    message: format!(
                        "field '{}' is type number but value '{}' is not numeric",
                        field.key, raw
                    ),
                    suggestion: "Provide a decimal number, e.g. 42 or 3.14".into(),
                }),
            "bool" => match raw.to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" => Ok(TypedFieldValue::Bool(true)),
                "false" | "no" | "0" => Ok(TypedFieldValue::Bool(false)),
                other => Err(AppError::BadInput {
                    code: "field_coercion_failed".into(),
                    message: format!(
                        "field '{}' is type bool but value '{}' is not boolean",
                        field.key, other
                    ),
                    suggestion: "Use true/false/yes/no/1/0".into(),
                }),
            },
            "date" => {
                // Accept plain YYYY-MM-DD first, normalizing to midnight
                // UTC RFC 3339. Fall through to the full RFC 3339 parser
                // for timestamps with time-of-day. Either way the stored
                // value is a normalized RFC 3339 string so downstream
                // string comparisons (`value_date >= ?`) line up.
                if let Ok(d) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
                    let dt = d.and_hms_opt(0, 0, 0).ok_or_else(|| AppError::Transient {
                        code: "date_hms_overflow".into(),
                        message: format!("could not build midnight timestamp for '{raw}'"),
                        suggestion: "report as a bug".into(),
                    })?;
                    let utc: chrono::DateTime<chrono::Utc> =
                        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc);
                    return Ok(TypedFieldValue::Date(utc.to_rfc3339()));
                }
                chrono::DateTime::parse_from_rfc3339(raw)
                    .map(|dt| TypedFieldValue::Date(dt.to_rfc3339()))
                    .map_err(|e| AppError::BadInput {
                        code: "field_coercion_failed".into(),
                        message: format!(
                            "field '{}' is type date but value '{}' is not RFC 3339: {e}",
                            field.key, raw
                        ),
                        suggestion:
                            "Use RFC 3339 or YYYY-MM-DD, e.g. 2026-04-08T12:00:00Z or 2026-04-08"
                                .into(),
                    })
            }
            "select" => {
                let options = field.options.as_ref().ok_or_else(|| AppError::Transient {
                    code: "select_without_options".into(),
                    message: format!("field '{}' is select but has no options", field.key),
                    suggestion: "Recreate the field with --options".into(),
                })?;
                if options.iter().any(|o| o == raw) {
                    Ok(TypedFieldValue::Text(raw.to_string()))
                } else {
                    Err(AppError::BadInput {
                        code: "field_coercion_failed".into(),
                        message: format!(
                            "field '{}' value '{}' is not in the allowed options",
                            field.key, raw
                        ),
                        suggestion: format!("Allowed options: {}", options.join(", ")),
                    })
                }
            }
            other => Err(AppError::Transient {
                code: "unknown_field_type".into(),
                message: format!("field '{}' has unknown type '{other}'", field.key),
                suggestion: "Inspect the field row — schema may be corrupt".into(),
            }),
        }
    }

    /// Write a typed value to `contact_field_value`. INSERT OR REPLACE so the
    /// caller doesn't need to check existence first.
    pub fn contact_field_upsert(
        &self,
        contact_id: i64,
        field_id: i64,
        value: &TypedFieldValue,
    ) -> Result<(), AppError> {
        let (text, num, date, b) = match value {
            TypedFieldValue::Text(s) => (Some(s.clone()), None, None, None),
            TypedFieldValue::Number(n) => (None, Some(*n), None, None),
            TypedFieldValue::Date(d) => (None, None, Some(d.clone()), None),
            TypedFieldValue::Bool(b) => (None, None, None, Some(if *b { 1i64 } else { 0 })),
        };
        self.conn
            .execute(
                "INSERT OR REPLACE INTO contact_field_value
                 (contact_id, field_id, value_text, value_number, value_date, value_bool)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![contact_id, field_id, text, num, date, b],
            )
            .map_err(query_err)?;
        Ok(())
    }

    /// Return all list names (and ids) the contact is currently a member of.
    pub fn contact_lists_for(&self, contact_id: i64) -> Result<Vec<(i64, String)>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT l.id, l.name FROM list l
                 JOIN list_membership lm ON lm.list_id = l.id
                 WHERE lm.contact_id = ?1
                 ORDER BY l.name",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map(params![contact_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    /// Fetch the full `Contact` row for a lookup by email.
    pub fn contact_get_by_email(&self, email: &str) -> Result<Option<Contact>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, email, first_name, last_name, status, created_at
                 FROM contact WHERE email = ?1 COLLATE NOCASE",
            )
            .map_err(query_err)?;
        let row = stmt.query_row(params![email], |row| {
            Ok(Contact {
                id: row.get(0)?,
                email: row.get(1)?,
                first_name: row.get(2)?,
                last_name: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
            })
        });
        match row {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    /// Fetch all field values for a contact, returned as (key, display_string).
    pub fn contact_fields_for(&self, contact_id: i64) -> Result<Vec<(String, String)>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT f.key, f.type, cfv.value_text, cfv.value_number, cfv.value_date, cfv.value_bool
                 FROM contact_field_value cfv
                 JOIN field f ON cfv.field_id = f.id
                 WHERE cfv.contact_id = ?1
                 ORDER BY f.key",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map(params![contact_id], |row| {
                let key: String = row.get(0)?;
                let ty: String = row.get(1)?;
                let display = match ty.as_str() {
                    "text" | "select" => row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    "number" => row
                        .get::<_, Option<f64>>(3)?
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                    "date" => row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    "bool" => row
                        .get::<_, Option<i64>>(5)?
                        .map(|b| if b != 0 { "true" } else { "false" }.to_string())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                Ok((key, display))
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    // ─── Segment operations ────────────────────────────────────────────

    pub fn segment_create(&self, name: &str, filter_json: &str) -> Result<i64, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO segment (name, filter_json, created_at) VALUES (?1, ?2, ?3)",
                params![name, filter_json, now],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed") {
                    AppError::BadInput {
                        code: "segment_already_exists".into(),
                        message: format!("a segment named '{name}' already exists"),
                        suggestion: "Run `mailing-list-cli segment ls` to see existing segments"
                            .into(),
                    }
                } else {
                    query_err(e)
                }
            })?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn segment_all(&self) -> Result<Vec<crate::models::Segment>, AppError> {
        // member_count is computed lazily (see `segment_count_members`); here it is 0.
        // Callers that need counts must call `segment_count_members` separately.
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, filter_json, created_at FROM segment ORDER BY name ASC")
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::Segment {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    filter_json: row.get(2)?,
                    created_at: row.get(3)?,
                    member_count: 0,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    pub fn segment_get_by_name(
        &self,
        name: &str,
    ) -> Result<Option<crate::models::Segment>, AppError> {
        let row = self.conn.query_row(
            "SELECT id, name, filter_json, created_at FROM segment WHERE name = ?1",
            params![name],
            |row| {
                Ok(crate::models::Segment {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    filter_json: row.get(2)?,
                    created_at: row.get(3)?,
                    member_count: 0,
                })
            },
        );
        match row {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn segment_delete(&self, name: &str) -> Result<bool, AppError> {
        let affected = self
            .conn
            .execute("DELETE FROM segment WHERE name = ?1", params![name])
            .map_err(query_err)?;
        Ok(affected > 0)
    }

    /// Count contacts matching a pre-compiled SQL fragment. The fragment and
    /// params MUST be produced by `segment::compiler::to_sql_where`.
    pub fn segment_count_members(
        &self,
        sql_fragment: &str,
        params: &[rusqlite::types::Value],
    ) -> Result<i64, AppError> {
        let sql = format!("SELECT COUNT(*) FROM contact c WHERE {sql_fragment}");
        let count: i64 = self
            .conn
            .query_row(&sql, rusqlite::params_from_iter(params.iter()), |r| {
                r.get(0)
            })
            .map_err(query_err)?;
        Ok(count)
    }

    /// Return the list of contact emails matching a compiled SQL fragment,
    /// paginated. Stable order by `contact.id ASC`.
    pub fn segment_members(
        &self,
        sql_fragment: &str,
        params: &[rusqlite::types::Value],
        limit: usize,
        cursor: Option<i64>,
    ) -> Result<Vec<Contact>, AppError> {
        let mut sql = format!(
            "SELECT c.id, c.email, c.first_name, c.last_name, c.status, c.created_at
             FROM contact c WHERE ({sql_fragment})"
        );
        if cursor.is_some() {
            sql.push_str(" AND c.id > ?");
        }
        sql.push_str(" ORDER BY c.id ASC LIMIT ?");
        let mut stmt = self.conn.prepare(&sql).map_err(query_err)?;

        // Build the params: original params, then cursor (if any), then limit.
        let mut all: Vec<rusqlite::types::Value> = params.to_vec();
        if let Some(c) = cursor {
            all.push(rusqlite::types::Value::Integer(c));
        }
        all.push(rusqlite::types::Value::Integer(limit as i64));

        let rows = stmt
            .query_map(rusqlite::params_from_iter(all.iter()), |row| {
                Ok(Contact {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    first_name: row.get(2)?,
                    last_name: row.get(3)?,
                    status: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    // ─── Template operations ───────────────────────────────────────────

    pub fn template_upsert(
        &self,
        name: &str,
        subject: &str,
        html_source: &str,
    ) -> Result<i64, AppError> {
        if !is_snake_case(name) {
            return Err(AppError::BadInput {
                code: "invalid_template_name".into(),
                message: format!("template name '{name}' must be snake_case"),
                suggestion:
                    "Use lowercase letters, digits, and underscores only (e.g. `welcome_email`)"
                        .into(),
            });
        }
        let now = chrono::Utc::now().to_rfc3339();
        // If a template with this name exists, UPDATE; else INSERT.
        let existing = self.template_get_by_name(name)?;
        if let Some(t) = existing {
            self.conn
                .execute(
                    "UPDATE template SET subject = ?1, html_source = ?2, updated_at = ?3 WHERE id = ?4",
                    params![subject, html_source, now, t.id],
                )
                .map_err(query_err)?;
            Ok(t.id)
        } else {
            self.conn
                .execute(
                    "INSERT INTO template (name, subject, html_source, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![name, subject, html_source, now],
                )
                .map_err(query_err)?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    pub fn template_all(&self) -> Result<Vec<crate::models::Template>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, subject, html_source, created_at, updated_at
                 FROM template ORDER BY name ASC",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::Template {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    subject: row.get(2)?,
                    html_source: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    pub fn template_get_by_name(
        &self,
        name: &str,
    ) -> Result<Option<crate::models::Template>, AppError> {
        let row = self.conn.query_row(
            "SELECT id, name, subject, html_source, created_at, updated_at
             FROM template WHERE name = ?1",
            params![name],
            |row| {
                Ok(crate::models::Template {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    subject: row.get(2)?,
                    html_source: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        );
        match row {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn template_delete(&self, name: &str) -> Result<bool, AppError> {
        let affected = self
            .conn
            .execute("DELETE FROM template WHERE name = ?1", params![name])
            .map_err(query_err)?;
        Ok(affected > 0)
    }

    // ─── Broadcast operations ──────────────────────────────────────────

    #[allow(dead_code)]
    pub fn broadcast_create(
        &self,
        name: &str,
        template_id: i64,
        target_kind: &str,
        target_id: i64,
    ) -> Result<i64, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO broadcast (name, template_id, target_kind, target_id, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'draft', ?5)",
                params![name, template_id, target_kind, target_id, now],
            )
            .map_err(query_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    #[allow(dead_code)]
    pub fn broadcast_all(
        &self,
        status_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::models::Broadcast>, AppError> {
        let (sql, has_status) = if status_filter.is_some() {
            (
                "SELECT id, name, template_id, target_kind, target_id, status, scheduled_at, sent_at, created_at,
                        recipient_count, delivered_count, bounced_count, opened_count, clicked_count,
                        unsubscribed_count, complained_count
                 FROM broadcast WHERE status = ?1 ORDER BY id DESC LIMIT ?2",
                true,
            )
        } else {
            (
                "SELECT id, name, template_id, target_kind, target_id, status, scheduled_at, sent_at, created_at,
                        recipient_count, delivered_count, bounced_count, opened_count, clicked_count,
                        unsubscribed_count, complained_count
                 FROM broadcast ORDER BY id DESC LIMIT ?1",
                false,
            )
        };
        let mut stmt = self.conn.prepare(sql).map_err(query_err)?;

        let row_mapper = |row: &rusqlite::Row| {
            Ok(crate::models::Broadcast {
                id: row.get(0)?,
                name: row.get(1)?,
                template_id: row.get(2)?,
                target_kind: row.get(3)?,
                target_id: row.get(4)?,
                status: row.get(5)?,
                scheduled_at: row.get(6)?,
                sent_at: row.get(7)?,
                created_at: row.get(8)?,
                recipient_count: row.get(9)?,
                delivered_count: row.get(10)?,
                bounced_count: row.get(11)?,
                opened_count: row.get(12)?,
                clicked_count: row.get(13)?,
                unsubscribed_count: row.get(14)?,
                complained_count: row.get(15)?,
            })
        };

        let rows: Vec<crate::models::Broadcast> = if has_status {
            stmt.query_map(params![status_filter.unwrap(), limit as i64], row_mapper)
                .map_err(query_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(query_err)?
        } else {
            stmt.query_map(params![limit as i64], row_mapper)
                .map_err(query_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(query_err)?
        };
        Ok(rows)
    }

    #[allow(dead_code)]
    pub fn broadcast_get(&self, id: i64) -> Result<Option<crate::models::Broadcast>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, template_id, target_kind, target_id, status, scheduled_at, sent_at, created_at,
                        recipient_count, delivered_count, bounced_count, opened_count, clicked_count,
                        unsubscribed_count, complained_count
                 FROM broadcast WHERE id = ?1",
            )
            .map_err(query_err)?;
        let row = stmt.query_row(params![id], |row| {
            Ok(crate::models::Broadcast {
                id: row.get(0)?,
                name: row.get(1)?,
                template_id: row.get(2)?,
                target_kind: row.get(3)?,
                target_id: row.get(4)?,
                status: row.get(5)?,
                scheduled_at: row.get(6)?,
                sent_at: row.get(7)?,
                created_at: row.get(8)?,
                recipient_count: row.get(9)?,
                delivered_count: row.get(10)?,
                bounced_count: row.get(11)?,
                opened_count: row.get(12)?,
                clicked_count: row.get(13)?,
                unsubscribed_count: row.get(14)?,
                complained_count: row.get(15)?,
            })
        });
        match row {
            Ok(b) => Ok(Some(b)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    #[allow(dead_code)]
    pub fn broadcast_set_status(
        &self,
        id: i64,
        status: &str,
        sent_at: Option<&str>,
    ) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE broadcast SET status = ?1, sent_at = COALESCE(?2, sent_at) WHERE id = ?3",
                params![status, sent_at, id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn broadcast_set_scheduled(&self, id: i64, scheduled_at: &str) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE broadcast SET status = 'scheduled', scheduled_at = ?1 WHERE id = ?2",
                params![scheduled_at, id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn broadcast_update_counts(&self, id: i64, recipient_count: i64) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE broadcast SET recipient_count = ?1 WHERE id = ?2",
                params![recipient_count, id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    // ─── Broadcast recipient operations ────────────────────────────────

    #[allow(dead_code)]
    pub fn broadcast_recipient_insert(
        &self,
        broadcast_id: i64,
        contact_id: i64,
        status: &str,
    ) -> Result<i64, AppError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO broadcast_recipient (broadcast_id, contact_id, status)
                 VALUES (?1, ?2, ?3)",
                params![broadcast_id, contact_id, status],
            )
            .map_err(query_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    #[allow(dead_code)]
    pub fn broadcast_recipient_mark_sent(
        &self,
        broadcast_id: i64,
        contact_id: i64,
        resend_email_id: &str,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE broadcast_recipient
                 SET status = 'sent', resend_email_id = ?1, sent_at = ?2
                 WHERE broadcast_id = ?3 AND contact_id = ?4",
                params![resend_email_id, now, broadcast_id, contact_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn broadcast_recipient_count_by_status(
        &self,
        broadcast_id: i64,
        status: &str,
    ) -> Result<i64, AppError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM broadcast_recipient WHERE broadcast_id = ?1 AND status = ?2",
                params![broadcast_id, status],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        Ok(count)
    }

    /// Check if an email is on the global suppression list.
    #[allow(dead_code)]
    pub fn is_email_suppressed(&self, email: &str) -> Result<bool, AppError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM suppression WHERE email = ?1 COLLATE NOCASE",
                params![email],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        Ok(count > 0)
    }

    /// Resolve a segment by its primary key id.
    #[allow(dead_code)]
    pub fn segment_get_by_id(&self, id: i64) -> Result<Option<crate::models::Segment>, AppError> {
        let row = self.conn.query_row(
            "SELECT id, name, filter_json, created_at FROM segment WHERE id = ?1",
            params![id],
            |row| {
                Ok(crate::models::Segment {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    filter_json: row.get(2)?,
                    created_at: row.get(3)?,
                    member_count: 0,
                })
            },
        );
        match row {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    // ─── Event operations ──────────────────────────────────────────────

    /// Insert an event row. Returns true if inserted, false if already
    /// present (idempotent via the unique index on (resend_email_id, type)).
    pub fn event_insert(
        &self,
        event_type: &str,
        resend_email_id: &str,
        broadcast_id: Option<i64>,
        contact_id: Option<i64>,
        payload_json: &str,
    ) -> Result<bool, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        let affected = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO event (type, resend_email_id, broadcast_id, contact_id, payload_json, received_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![event_type, resend_email_id, broadcast_id, contact_id, payload_json, now],
            )
            .map_err(query_err)?;
        Ok(affected > 0)
    }

    /// Look up a `broadcast_recipient` row by `resend_email_id`.
    pub fn recipient_by_resend_email_id(
        &self,
        resend_email_id: &str,
    ) -> Result<Option<(i64, i64)>, AppError> {
        match self.conn.query_row(
            "SELECT broadcast_id, contact_id FROM broadcast_recipient WHERE resend_email_id = ?1",
            params![resend_email_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        ) {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn broadcast_recipient_update_status(
        &self,
        broadcast_id: i64,
        contact_id: i64,
        status: &str,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE broadcast_recipient
                 SET status = ?1, last_event_at = ?2
                 WHERE broadcast_id = ?3 AND contact_id = ?4",
                params![status, now, broadcast_id, contact_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn broadcast_increment_stat(
        &self,
        broadcast_id: i64,
        column: &str,
    ) -> Result<(), AppError> {
        // Whitelist the column names to prevent SQL injection
        let column = match column {
            "delivered_count" => "delivered_count",
            "bounced_count" => "bounced_count",
            "opened_count" => "opened_count",
            "clicked_count" => "clicked_count",
            "unsubscribed_count" => "unsubscribed_count",
            "complained_count" => "complained_count",
            _ => {
                return Err(AppError::BadInput {
                    code: "bad_stat_column".into(),
                    message: format!("unknown stat column: {column}"),
                    suggestion: "Report as a bug".into(),
                });
            }
        };
        let sql = format!("UPDATE broadcast SET {column} = {column} + 1 WHERE id = ?1");
        self.conn
            .execute(&sql, params![broadcast_id])
            .map_err(query_err)?;
        Ok(())
    }

    pub fn click_insert(
        &self,
        broadcast_id: i64,
        contact_id: Option<i64>,
        link: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO click (broadcast_id, contact_id, link, ip_address, user_agent, clicked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![broadcast_id, contact_id, link, ip_address, user_agent, now],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn suppression_insert(
        &self,
        email: &str,
        reason: &str,
        source_broadcast_id: Option<i64>,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR IGNORE INTO suppression (email, reason, suppressed_at, source_broadcast_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![email, reason, now, source_broadcast_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    /// Return the set of contact IDs already marked `sent` for the given
    /// broadcast. Used by the `broadcast send` / `broadcast resume` pipeline
    /// to skip recipients already processed in a previous interrupted run —
    /// the v0.3 replacement for "re-render every chunk and rely on INSERT
    /// OR IGNORE dedup" which wasted Resend API calls at scale.
    pub fn broadcast_recipient_already_sent_ids(
        &self,
        broadcast_id: i64,
    ) -> Result<std::collections::HashSet<i64>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT contact_id FROM broadcast_recipient
                 WHERE broadcast_id = ?1 AND status = 'sent'",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map([broadcast_id], |row| row.get::<_, i64>(0))
            .map_err(query_err)?;
        let mut set = std::collections::HashSet::new();
        for row in rows {
            set.insert(row.map_err(query_err)?);
        }
        Ok(set)
    }

    /// Load the entire suppression list into an in-memory `HashSet<String>`
    /// keyed by lowercased email. Used by the send pipeline to replace per-
    /// recipient `is_email_suppressed` queries with O(1) lookups. Worth the
    /// trade-off any time the send size is > ~100 recipients.
    ///
    /// Normalization: the `suppression.email` column uses COLLATE NOCASE in
    /// SQLite, so we lowercase on the Rust side to preserve that semantics
    /// when the caller does a `set.contains(&email.to_ascii_lowercase())`.
    pub fn suppression_all_emails(&self) -> Result<std::collections::HashSet<String>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT email FROM suppression")
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(query_err)?;
        let mut set = std::collections::HashSet::new();
        for row in rows {
            let email = row.map_err(query_err)?;
            set.insert(email.to_ascii_lowercase());
        }
        Ok(set)
    }

    /// GDPR Article 17 "Right to erasure" primitive. Deletes the contact
    /// row (cascades to list_membership, contact_tag, contact_field_value,
    /// broadcast_recipient, soft_bounce_count, optin_token via FOREIGN KEY
    /// ON DELETE CASCADE) and inserts a suppression tombstone with
    /// reason='gdpr_erasure' so that any future `contact add` of the same
    /// email is blocked pre-send by the suppression filter.
    ///
    /// Runs inside a transaction: either all writes succeed or none do.
    /// The suppression tombstone is inserted BEFORE the contact delete so
    /// the row cannot be momentarily absent from both places.
    pub fn contact_erase(&mut self, email: &str) -> Result<(), AppError> {
        // Look up first so we return a clear error if the contact doesn't
        // exist. (Erase is intentionally NOT idempotent via "if exists" —
        // the agent needs to know if they typoed the email.)
        if self.contact_get_by_email(email)?.is_none() {
            return Err(AppError::BadInput {
                code: "contact_not_found".into(),
                message: format!("no contact with email '{email}'"),
                suggestion: "Check the email spelling".into(),
            });
        }

        let tx = self.conn.transaction().map_err(|e| AppError::Transient {
            code: "tx_begin_failed".into(),
            message: format!("begin erase transaction failed: {e}"),
            suggestion: "Retry — the DB is probably busy".into(),
        })?;

        let now = chrono::Utc::now().to_rfc3339();
        // 1. Insert the suppression tombstone BEFORE deleting the contact.
        tx.execute(
            "INSERT OR REPLACE INTO suppression (email, reason, suppressed_at, source_broadcast_id)
             VALUES (?1, 'gdpr_erasure', ?2, NULL)",
            rusqlite::params![email, now],
        )
        .map_err(query_err)?;

        // 2. Delete the contact row. FK cascades handle all child tables.
        tx.execute(
            "DELETE FROM contact WHERE email = ?1 COLLATE NOCASE",
            rusqlite::params![email],
        )
        .map_err(query_err)?;

        tx.commit().map_err(|e| AppError::Transient {
            code: "tx_commit_failed".into(),
            message: format!("commit erase transaction failed: {e}"),
            suggestion: "Retry — the DB is probably busy".into(),
        })?;
        Ok(())
    }

    pub fn contact_set_status(&self, email: &str, status: &str) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE contact SET status = ?1, updated_at = ?2 WHERE email = ?3 COLLATE NOCASE",
                params![status, chrono::Utc::now().to_rfc3339(), email],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn soft_bounce_increment(&self, contact_id: i64) -> Result<i64, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO soft_bounce_count (contact_id, consecutive, last_bounce_at)
                 VALUES (?1, 1, ?2)
                 ON CONFLICT(contact_id) DO UPDATE SET consecutive = consecutive + 1, last_bounce_at = ?2",
                params![contact_id, now],
            )
            .map_err(query_err)?;
        let count: i64 = self
            .conn
            .query_row(
                "SELECT consecutive FROM soft_bounce_count WHERE contact_id = ?1",
                params![contact_id],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        Ok(count)
    }

    pub fn soft_bounce_reset(&self, contact_id: i64) -> Result<(), AppError> {
        self.conn
            .execute(
                "DELETE FROM soft_bounce_count WHERE contact_id = ?1",
                params![contact_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    // ─── KV cursor operations ──────────────────────────────────────────

    #[allow(dead_code)]
    pub fn kv_get(&self, key: &str) -> Result<Option<String>, AppError> {
        match self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            }) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    #[allow(dead_code)]
    pub fn kv_set(&self, key: &str, value: &str) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO kv (key, value, updated_at) VALUES (?1, ?2, ?3)",
                params![key, value, now],
            )
            .map_err(query_err)?;
        Ok(())
    }

    // ─── Report aggregations ───────────────────────────────────────────

    pub fn report_summary(
        &self,
        broadcast_id: i64,
    ) -> Result<crate::models::ReportSummary, AppError> {
        let broadcast = self
            .broadcast_get(broadcast_id)?
            .ok_or_else(|| AppError::BadInput {
                code: "broadcast_not_found".into(),
                message: format!("no broadcast with id {broadcast_id}"),
                suggestion: "Run `mailing-list-cli broadcast ls`".into(),
            })?;
        let suppressed_count =
            self.broadcast_recipient_count_by_status(broadcast_id, "suppressed")?;

        let ctr = if broadcast.delivered_count > 0 {
            (broadcast.clicked_count as f64 / broadcast.delivered_count as f64) * 100.0
        } else {
            0.0
        };
        let open_rate = if broadcast.delivered_count > 0 {
            (broadcast.opened_count as f64 / broadcast.delivered_count as f64) * 100.0
        } else {
            0.0
        };
        let total_sent = (broadcast.recipient_count - suppressed_count).max(0);
        let bounce_rate = if total_sent > 0 {
            (broadcast.bounced_count as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };
        let complaint_rate = if total_sent > 0 {
            (broadcast.complained_count as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };

        Ok(crate::models::ReportSummary {
            broadcast_id,
            broadcast_name: broadcast.name,
            recipient_count: broadcast.recipient_count,
            delivered_count: broadcast.delivered_count,
            bounced_count: broadcast.bounced_count,
            opened_count: broadcast.opened_count,
            clicked_count: broadcast.clicked_count,
            unsubscribed_count: broadcast.unsubscribed_count,
            complained_count: broadcast.complained_count,
            suppressed_count,
            ctr,
            bounce_rate,
            complaint_rate,
            open_rate,
        })
    }

    pub fn report_links(
        &self,
        broadcast_id: i64,
    ) -> Result<Vec<crate::models::LinkReport>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT link, COUNT(*) as clicks, COUNT(DISTINCT contact_id) as unique_clickers
                 FROM click WHERE broadcast_id = ?1
                 GROUP BY link ORDER BY clicks DESC",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map(params![broadcast_id], |row| {
                Ok(crate::models::LinkReport {
                    link: row.get(0)?,
                    clicks: row.get(1)?,
                    unique_clickers: row.get(2)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    /// Compute complaint + bounce rate over the last `days` days based on
    /// the `event` table. Returns `(complaint_rate, bounce_rate, delivered)`
    /// where both rates are fractions (0.005 = 0.5%) over the delivered
    /// count in the same window. Returns `(0.0, 0.0, 0)` if there are no
    /// delivered events in the window.
    ///
    /// v0.3: used by the broadcast send preflight to refuse sends that
    /// would worsen domain reputation. Unlike `report_deliverability`,
    /// which aggregates from broadcast columns, this queries the `event`
    /// table directly so it reflects the actual webhook stream — which
    /// is what Gmail/Yahoo use to score your reputation in real time.
    pub fn historical_send_rates(&self, days: i64) -> Result<(f64, f64, i64), AppError> {
        let since = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let delivered: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM event
                 WHERE type = 'email.delivered' AND received_at >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        if delivered == 0 {
            return Ok((0.0, 0.0, 0));
        }
        let complained: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM event
                 WHERE type = 'email.complained' AND received_at >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        let bounced: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM event
                 WHERE type = 'email.bounced' AND received_at >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        Ok((
            complained as f64 / delivered as f64,
            bounced as f64 / delivered as f64,
            delivered,
        ))
    }

    pub fn report_deliverability(
        &self,
        window_days: i64,
    ) -> Result<crate::models::DeliverabilityReport, AppError> {
        let since = chrono::Utc::now() - chrono::Duration::days(window_days);
        let since_str = since.to_rfc3339();

        let (total_sent, total_delivered, total_bounced, total_complained): (i64, i64, i64, i64) =
            self.conn
                .query_row(
                    "SELECT
                        COALESCE(SUM(recipient_count), 0),
                        COALESCE(SUM(delivered_count), 0),
                        COALESCE(SUM(bounced_count), 0),
                        COALESCE(SUM(complained_count), 0)
                     FROM broadcast WHERE created_at >= ?1",
                    params![since_str],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(query_err)?;

        let bounce_rate = if total_sent > 0 {
            (total_bounced as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };
        let complaint_rate = if total_sent > 0 {
            (total_complained as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };

        Ok(crate::models::DeliverabilityReport {
            window_days,
            total_sent,
            total_delivered,
            total_bounced,
            total_complained,
            bounce_rate,
            complaint_rate,
            verified_domains: vec![], // Phase 6 doesn't wire domain list; Phase 7 dnscheck does
        })
    }
}

/// Stored consent record for a contact. Both fields may be `None` if
/// consent was never recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactConsent {
    pub source: Option<String>,
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypedFieldValue {
    Text(String),
    Number(f64),
    Date(String), // normalized RFC 3339 string
    Bool(bool),
}

fn is_snake_case(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.starts_with('_')
        && !s.ends_with('_')
}

fn query_err(e: rusqlite::Error) -> AppError {
    AppError::Transient {
        code: "db_query_failed".into(),
        message: format!("database query failed: {e}"),
        suggestion: "Run `mailing-list-cli health` to inspect the database state".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_all_tables() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let table_count: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            table_count >= 17,
            "expected at least 17 tables, got {table_count}"
        );
    }

    #[test]
    fn migration_is_idempotent() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let _ = Db::open_at(tmp.path()).unwrap();
        let _ = Db::open_at(tmp.path()).unwrap();
        let _ = Db::open_at(tmp.path()).unwrap();
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let fk: i64 = db
            .conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn list_create_and_list_all() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let id = db
            .list_create("newsletter", Some("weekly digest"), "aud_abc123")
            .unwrap();
        assert!(id > 0);
        let lists = db.list_all().unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].name, "newsletter");
        assert_eq!(lists[0].member_count, 0);
    }

    #[test]
    fn list_create_duplicate_returns_bad_input() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.list_create("foo", None, "aud_1").unwrap();
        let err = db.list_create("foo", None, "aud_2").unwrap_err();
        assert_eq!(err.code(), "list_already_exists");
    }

    #[test]
    fn contact_upsert_is_idempotent() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let id1 = db
            .contact_upsert("alice@example.com", Some("Alice"), None)
            .unwrap();
        let id2 = db
            .contact_upsert("alice@example.com", Some("Alice"), None)
            .unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn contact_add_to_list_then_list_in_list() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let list_id = db.list_create("vip", None, "aud_v").unwrap();
        let alice = db
            .contact_upsert("alice@example.com", Some("Alice"), None)
            .unwrap();
        let bob = db
            .contact_upsert("bob@example.com", Some("Bob"), None)
            .unwrap();
        db.contact_add_to_list(alice, list_id).unwrap();
        db.contact_add_to_list(bob, list_id).unwrap();
        // Adding same contact again is a no-op
        db.contact_add_to_list(alice, list_id).unwrap();
        let contacts = db.contact_list_in_list(list_id, 100).unwrap();
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].email, "alice@example.com");
        assert_eq!(contacts[1].email, "bob@example.com");

        // member_count reflects the additions
        let list = db.list_get_by_id(list_id).unwrap().unwrap();
        assert_eq!(list.member_count, 2);
    }

    #[test]
    fn tag_get_or_create_is_idempotent() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let id1 = db.tag_get_or_create("vip").unwrap();
        let id2 = db.tag_get_or_create("vip").unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn tag_all_sorts_by_name_with_counts() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.tag_get_or_create("vip").unwrap();
        db.tag_get_or_create("abandoned").unwrap();
        let tags = db.tag_all().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "abandoned");
        assert_eq!(tags[1].name, "vip");
        assert_eq!(tags[0].member_count, 0);
    }

    #[test]
    fn contact_tag_add_and_remove_round_trip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let contact = db.contact_upsert("alice@example.com", None, None).unwrap();
        let tag = db.tag_get_or_create("vip").unwrap();
        db.contact_tag_add(contact, tag).unwrap();
        assert_eq!(
            db.contact_tags_for(contact).unwrap(),
            vec!["vip".to_string()]
        );
        // Idempotent add
        db.contact_tag_add(contact, tag).unwrap();
        assert_eq!(db.contact_tags_for(contact).unwrap().len(), 1);
        // Remove
        assert!(db.contact_tag_remove(contact, tag).unwrap());
        assert!(db.contact_tags_for(contact).unwrap().is_empty());
    }

    #[test]
    fn tag_delete_cascades_to_contact_tag() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let contact = db.contact_upsert("alice@example.com", None, None).unwrap();
        let tag_id = db.tag_get_or_create("vip").unwrap();
        db.contact_tag_add(contact, tag_id).unwrap();
        assert!(db.tag_delete("vip").unwrap());
        assert!(db.contact_tags_for(contact).unwrap().is_empty());
    }

    #[test]
    fn field_create_validates_snake_case() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        assert!(db.field_create("Company Size", "text", None).is_err());
        assert!(db.field_create("company-size", "text", None).is_err());
        assert!(db.field_create("company_size", "text", None).is_ok());
    }

    #[test]
    fn field_create_select_requires_options() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        assert!(db.field_create("plan", "select", None).is_err());
        assert!(
            db.field_create("plan", "select", Some(&["free".into(), "pro".into()]))
                .is_ok()
        );
    }

    #[test]
    fn field_all_sorted_by_key_with_options() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("zebra", "text", None).unwrap();
        db.field_create("apple", "select", Some(&["a".into(), "b".into()]))
            .unwrap();
        let fields = db.field_all().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].key, "apple");
        assert_eq!(
            fields[0].options.as_ref().unwrap(),
            &vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn field_delete_removes_and_cascades() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("company", "text", None).unwrap();
        assert!(db.field_delete("company").unwrap());
        assert!(db.field_get("company").unwrap().is_none());
    }

    #[test]
    fn coerce_number_accepts_integers_and_decimals() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("age", "number", None).unwrap();
        let f = db.field_get("age").unwrap().unwrap();
        assert_eq!(
            db.coerce_field_value(&f, "42").unwrap(),
            TypedFieldValue::Number(42.0)
        );
        assert_eq!(
            db.coerce_field_value(&f, "2.5").unwrap(),
            TypedFieldValue::Number(2.5)
        );
    }

    #[test]
    fn coerce_number_rejects_text() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("age", "number", None).unwrap();
        let f = db.field_get("age").unwrap().unwrap();
        assert!(db.coerce_field_value(&f, "old").is_err());
    }

    #[test]
    fn coerce_date_accepts_plain_date_and_rfc3339() {
        // Bug 6 regression: the `date` field type used to require a full
        // RFC 3339 timestamp, rejecting plain dates like `2026-04-08`.
        // Both forms must now parse and normalize to RFC 3339.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("event_date", "date", None).unwrap();
        let f = db.field_get("event_date").unwrap().unwrap();

        // Plain date
        match db.coerce_field_value(&f, "2026-04-08").unwrap() {
            TypedFieldValue::Date(s) => {
                assert!(
                    s.starts_with("2026-04-08T00:00:00"),
                    "plain date normalized to midnight RFC3339, got {s}"
                );
                assert!(s.ends_with("+00:00") || s.ends_with("Z"));
            }
            other => panic!("expected Date, got {other:?}"),
        }

        // Existing RFC 3339 path still works
        match db.coerce_field_value(&f, "2026-04-08T12:34:56Z").unwrap() {
            TypedFieldValue::Date(s) => assert!(s.contains("2026-04-08T12:34:56")),
            other => panic!("expected Date, got {other:?}"),
        }

        // Garbage still rejected
        assert!(db.coerce_field_value(&f, "not-a-date").is_err());
    }

    #[test]
    fn coerce_bool_accepts_common_forms() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("subscribed", "bool", None).unwrap();
        let f = db.field_get("subscribed").unwrap().unwrap();
        for truthy in &["true", "TRUE", "yes", "1"] {
            assert_eq!(
                db.coerce_field_value(&f, truthy).unwrap(),
                TypedFieldValue::Bool(true)
            );
        }
        for falsy in &["false", "NO", "0"] {
            assert_eq!(
                db.coerce_field_value(&f, falsy).unwrap(),
                TypedFieldValue::Bool(false)
            );
        }
    }

    #[test]
    fn coerce_select_enforces_options() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("plan", "select", Some(&["free".into(), "pro".into()]))
            .unwrap();
        let f = db.field_get("plan").unwrap().unwrap();
        assert!(db.coerce_field_value(&f, "pro").is_ok());
        assert!(db.coerce_field_value(&f, "enterprise").is_err());
    }

    #[test]
    fn contact_field_upsert_and_read_back() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let c = db.contact_upsert("alice@example.com", None, None).unwrap();
        let _ = db.field_create("company", "text", None).unwrap();
        let f = db.field_get("company").unwrap().unwrap();
        db.contact_field_upsert(c, f.id, &TypedFieldValue::Text("Acme".into()))
            .unwrap();
        let values = db.contact_fields_for(c).unwrap();
        assert_eq!(values, vec![("company".to_string(), "Acme".to_string())]);
        // Overwrite
        db.contact_field_upsert(c, f.id, &TypedFieldValue::Text("Globex".into()))
            .unwrap();
        let values = db.contact_fields_for(c).unwrap();
        assert_eq!(values[0].1, "Globex");
    }

    #[test]
    fn segment_crud_round_trip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let id = db.segment_create("vips", "{}").unwrap();
        assert!(id > 0);
        let all = db.segment_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "vips");
        assert!(db.segment_get_by_name("vips").unwrap().is_some());
        assert!(db.segment_delete("vips").unwrap());
        assert!(db.segment_all().unwrap().is_empty());
    }

    #[test]
    fn segment_duplicate_name_errors() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.segment_create("a", "{}").unwrap();
        assert_eq!(
            db.segment_create("a", "{}").unwrap_err().code(),
            "segment_already_exists"
        );
    }

    #[test]
    fn template_crud_round_trip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let id = db.template_upsert("welcome", "Hi", "<p>Hi</p>").unwrap();
        assert!(id > 0);
        let fetched = db.template_get_by_name("welcome").unwrap().unwrap();
        assert_eq!(fetched.subject, "Hi");

        // Upsert updates the existing row
        let id2 = db.template_upsert("welcome", "Hello", "<p>Hi</p>").unwrap();
        assert_eq!(id, id2);
        let updated = db.template_get_by_name("welcome").unwrap().unwrap();
        assert_eq!(updated.subject, "Hello");

        let all = db.template_all().unwrap();
        assert_eq!(all.len(), 1);

        assert!(db.template_delete("welcome").unwrap());
        assert!(db.template_get_by_name("welcome").unwrap().is_none());
    }

    #[test]
    fn template_rejects_non_snake_case_name() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        assert!(
            db.template_upsert("WelcomeEmail", "Hi", "<p>Hi</p>")
                .is_err()
        );
        assert!(
            db.template_upsert("welcome-email", "Hi", "<p>Hi</p>")
                .is_err()
        );
        assert!(
            db.template_upsert("welcome_email", "Hi", "<p>Hi</p>")
                .is_ok()
        );
    }

    #[test]
    fn broadcast_crud_round_trip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        // Need a template to satisfy FK
        let tid = db.template_upsert("t", "Hi", "<p>Hi</p>").unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();

        let bid = db.broadcast_create("Q1", tid, "list", list_id).unwrap();
        assert!(bid > 0);
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.name, "Q1");
        assert_eq!(b.status, "draft");

        db.broadcast_set_status(bid, "sending", None).unwrap();
        db.broadcast_set_status(bid, "sent", Some("2026-04-08T12:00:00Z"))
            .unwrap();
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.status, "sent");
        assert_eq!(b.sent_at.as_deref(), Some("2026-04-08T12:00:00Z"));

        let all = db.broadcast_all(None, 100).unwrap();
        assert_eq!(all.len(), 1);
        let sent = db.broadcast_all(Some("sent"), 100).unwrap();
        assert_eq!(sent.len(), 1);
        let draft = db.broadcast_all(Some("draft"), 100).unwrap();
        assert_eq!(draft.len(), 0);
    }

    #[test]
    fn broadcast_recipient_crud() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let tid = db.template_upsert("t", "Hi", "<p>Hi</p>").unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();
        let bid = db.broadcast_create("Q1", tid, "list", list_id).unwrap();
        let cid = db.contact_upsert("alice@example.com", None, None).unwrap();

        db.broadcast_recipient_insert(bid, cid, "pending").unwrap();
        db.broadcast_recipient_mark_sent(bid, cid, "em_abc")
            .unwrap();
        assert_eq!(
            db.broadcast_recipient_count_by_status(bid, "sent").unwrap(),
            1
        );
    }

    #[test]
    fn suppression_read_check() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.conn
            .execute(
                "INSERT INTO suppression (email, reason, suppressed_at) VALUES ('blocked@example.com', 'hard_bounced', '2026-01-01')",
                [],
            )
            .unwrap();
        assert!(db.is_email_suppressed("blocked@example.com").unwrap());
        assert!(db.is_email_suppressed("BLOCKED@example.com").unwrap()); // COLLATE NOCASE
        assert!(!db.is_email_suppressed("alice@example.com").unwrap());
    }

    #[test]
    fn suppression_all_emails_returns_normalized_set() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.suppression_insert("ALICE@example.com", "hard_bounced", None)
            .unwrap();
        db.suppression_insert("bob@example.com", "unsubscribed", None)
            .unwrap();
        // Insert via raw SQL too to cover collation edge cases.
        db.conn
            .execute(
                "INSERT INTO suppression (email, reason, suppressed_at) VALUES ('Carol@Example.com', 'complained', '2026-01-01')",
                [],
            )
            .unwrap();

        let set = db.suppression_all_emails().unwrap();
        assert_eq!(set.len(), 3);
        // All three lookups hit regardless of the original casing, because
        // the set is keyed by lowercased email and the caller is expected
        // to lowercase on lookup too.
        assert!(set.contains("alice@example.com"));
        assert!(set.contains("bob@example.com"));
        assert!(set.contains("carol@example.com"));
        // Sanity: the raw uppercased forms are NOT in the set, proving we
        // normalized on insert rather than relying on COLLATE NOCASE.
        assert!(!set.contains("ALICE@example.com"));
    }

    #[test]
    fn suppression_all_emails_on_empty_table() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let set = db.suppression_all_emails().unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn historical_send_rates_returns_window_rates() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        // Seed 1000 delivered events and 5 complained events in the last
        // 30 days. Expect complaint_rate = 0.005, bounce_rate = 0.
        let now = chrono::Utc::now().to_rfc3339();
        for i in 0..1_000 {
            db.conn
                .execute(
                    "INSERT INTO event (type, resend_email_id, payload_json, received_at)
                     VALUES ('email.delivered', ?1, '{}', ?2)",
                    rusqlite::params![format!("em_d_{i}"), now],
                )
                .unwrap();
        }
        for i in 0..5 {
            db.conn
                .execute(
                    "INSERT INTO event (type, resend_email_id, payload_json, received_at)
                     VALUES ('email.complained', ?1, '{}', ?2)",
                    rusqlite::params![format!("em_c_{i}"), now],
                )
                .unwrap();
        }
        let (complaint_rate, bounce_rate, delivered) = db.historical_send_rates(30).unwrap();
        assert_eq!(delivered, 1_000);
        assert!(
            (complaint_rate - 0.005).abs() < 1e-6,
            "complaint_rate: {complaint_rate}"
        );
        assert!(bounce_rate.abs() < 1e-6, "bounce_rate: {bounce_rate}");
    }

    #[test]
    fn historical_send_rates_empty_window_returns_zero() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let (complaint_rate, bounce_rate, delivered) = db.historical_send_rates(30).unwrap();
        assert_eq!(delivered, 0);
        assert_eq!(complaint_rate, 0.0);
        assert_eq!(bounce_rate, 0.0);
    }

    #[test]
    fn contact_erase_deletes_row_cascades_and_adds_gdpr_tombstone() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut db = Db::open_at(tmp.path()).unwrap();
        let list_id = db.list_create("l", None, "seg_x").unwrap();
        let cid = db
            .contact_upsert("alice@example.com", Some("Alice"), None)
            .unwrap();
        db.contact_add_to_list(cid, list_id).unwrap();
        let tag_id = db.tag_get_or_create("vip").unwrap();
        db.contact_tag_add(cid, tag_id).unwrap();

        // Erase.
        db.contact_erase("alice@example.com").unwrap();

        // Row is gone.
        assert!(
            db.contact_get_by_email("alice@example.com")
                .unwrap()
                .is_none(),
            "contact row should be deleted"
        );

        // FK cascade dropped the contact_tag row.
        let tag_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM contact_tag WHERE contact_id = ?1",
                [cid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tag_count, 0);

        // Cascade dropped the list_membership row too.
        let lm_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM list_membership WHERE contact_id = ?1",
                [cid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lm_count, 0);

        // Suppression tombstone present with the GDPR reason.
        let reason: String = db
            .conn
            .query_row(
                "SELECT reason FROM suppression WHERE email = ?1 COLLATE NOCASE",
                ["alice@example.com"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reason, "gdpr_erasure");

        // The HashSet-based lookup picks it up too (integrates with Task 2).
        let set = db.suppression_all_emails().unwrap();
        assert!(set.contains("alice@example.com"));
    }

    #[test]
    fn broadcast_recipient_already_sent_ids_returns_only_sent() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let tid = db.template_upsert("t", "s", "<p>h</p>").unwrap();
        let lid = db.list_create("l", None, "seg_x").unwrap();
        let bid = db.broadcast_create("b", tid, "list", lid).unwrap();
        let c_sent = db.contact_upsert("sent@ex.com", None, None).unwrap();
        let c_pending = db.contact_upsert("pending@ex.com", None, None).unwrap();
        let c_bounced = db.contact_upsert("bounced@ex.com", None, None).unwrap();

        db.broadcast_recipient_insert(bid, c_sent, "sent").unwrap();
        db.broadcast_recipient_insert(bid, c_pending, "pending")
            .unwrap();
        db.broadcast_recipient_insert(bid, c_bounced, "bounced")
            .unwrap();

        let sent_ids = db.broadcast_recipient_already_sent_ids(bid).unwrap();
        assert_eq!(sent_ids.len(), 1);
        assert!(sent_ids.contains(&c_sent));
        assert!(!sent_ids.contains(&c_pending));
        assert!(!sent_ids.contains(&c_bounced));
    }

    #[test]
    fn contact_erase_nonexistent_email_is_bad_input_error() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut db = Db::open_at(tmp.path()).unwrap();
        let err = db.contact_erase("nobody@example.com").unwrap_err();
        assert_eq!(err.code(), "contact_not_found");
    }

    #[test]
    fn historical_send_rates_excludes_old_events() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        // 100 delivered events FROM 60 DAYS AGO — outside the 30-day window.
        let old = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        for i in 0..100 {
            db.conn
                .execute(
                    "INSERT INTO event (type, resend_email_id, payload_json, received_at)
                     VALUES ('email.delivered', ?1, '{}', ?2)",
                    rusqlite::params![format!("em_old_{i}"), old],
                )
                .unwrap();
        }
        let (_, _, delivered) = db.historical_send_rates(30).unwrap();
        assert_eq!(delivered, 0, "old events should be excluded from window");
    }
}
