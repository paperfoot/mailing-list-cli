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
            "date" => chrono::DateTime::parse_from_rfc3339(raw)
                .map(|dt| TypedFieldValue::Date(dt.to_rfc3339()))
                .map_err(|e| AppError::BadInput {
                    code: "field_coercion_failed".into(),
                    message: format!(
                        "field '{}' is type date but value '{}' is not RFC 3339: {e}",
                        field.key, raw
                    ),
                    suggestion: "Use RFC 3339, e.g. 2026-04-08T12:00:00Z".into(),
                }),
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
}
