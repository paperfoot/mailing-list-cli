# Phase 3: Contacts, Tags, Fields, Segments — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `mailing-list-cli v0.0.4` — everything needed to define WHO a campaign will be sent to, before broadcasts land in Phase 5. This phase adds tags, typed custom fields, dynamic segments (saved boolean filters), the filter expression parser, CSV import with consent tracking, and read + mutation contact commands (`show`, `tag`, `untag`, `set`, `import`, `ls --filter`). **`contact erase` and `contact resubscribe` are deferred to Phase 7** — they can't ship cleanly without the suppression CRUD and audit log that Phase 7 owns. **`--double-opt-in` is a visible-but-rejected flag** in Phase 3; real DOI ships in Phase 7.

**Architecture:** A new `src/segment/` module owns the filter expression language: a pest grammar parses human-written filters into a `SegmentExpr` AST, serde serializes the AST to JSON for storage in `segment.filter_json`, and a compiler walks the AST to produce a parameterized SQL `WHERE` clause. Tags and fields are thin CRUD surfaces over pre-existing schema tables. CSV import streams rows, enforces `consent_source` per spec §9.3, checks each address against the global suppression list, rate-limits at the `EmailCli` subprocess layer (not at the row layer), and replays idempotently on crash. `segment members` and `contact ls --filter` share one code path so the two consumers never diverge.

**Tech Stack:** Rust 2024 edition, MSRV 1.85. New dependencies: `pest` 2.8, `pest_derive` 2.8, `csv` 1.4. Existing: `clap` 4.5, `rusqlite` 0.37 (bundled), `serde` 1.0, `serde_json`, `thiserror`, `chrono`.

**Spec reference:** [`docs/specs/2026-04-07-mailing-list-cli-design.md`](../specs/2026-04-07-mailing-list-cli-design.md), specifically §3 (data model), §4.2–4.4 (contacts/tags/fields/segments command surface), §6 (filter expression language), §8 (suppression semantics), §9.2 (GDPR erasure flow), §9.3 (consent record), §13 (email-cli interface).

**Parity plan reference:** [`docs/plans/2026-04-08-parity-plan.md`](./2026-04-08-parity-plan.md) §5 Phase 3.

**Prerequisite understanding:** The entire schema (tag, contact_tag, field, contact_field_value, segment, suppression, event, click tables) is **already in place** from migration `0001_initial`. Phase 3 is pure CLI surface + filter parser + business logic — no schema work. Verify with:

```bash
sqlite3 ~/.local/share/mailing-list-cli/state.db ".schema tag"
sqlite3 ~/.local/share/mailing-list-cli/state.db ".schema field"
sqlite3 ~/.local/share/mailing-list-cli/state.db ".schema segment"
```

---

## File structure

New files created by this phase:

```
mailing-list-cli/
├── src/
│   ├── segment/
│   │   ├── mod.rs              # module root; re-exports ast/parser/compiler
│   │   ├── grammar.pest        # pest grammar for filter expressions
│   │   ├── ast.rs              # SegmentExpr enum + Atom + serde
│   │   ├── parser.rs           # pest → SegmentExpr + ParseError
│   │   └── compiler.rs         # SegmentExpr → (SQL WHERE fragment, params)
│   ├── csv_import.rs           # Streaming CSV reader + consent validation
│   └── commands/
│       ├── tag.rs              # `tag ls`, `tag rm`
│       ├── field.rs            # `field create`, `field ls`, `field rm`
│       └── segment.rs          # `segment create/ls/show/members/rm`
```

Files modified:

```
├── Cargo.toml                  # +pest, +pest_derive, +csv
├── src/
│   ├── cli.rs                  # +Tag/Field/Segment variants; extend ContactAction
│   ├── commands/
│   │   ├── mod.rs              # +tag, +field, +segment modules
│   │   ├── contact.rs          # +show/tag/untag/set/erase/resubscribe/import; ls --filter
│   │   └── agent_info.rs       # list all new commands in the manifest
│   ├── db/mod.rs               # +tag_*, field_*, segment_*, contact_* helpers
│   ├── email_cli.rs            # +contact_update, +contact_delete
│   ├── main.rs                 # dispatch Tag/Field/Segment commands
│   └── models.rs               # +Tag, Field, Segment, ContactDetails structs
├── tests/
│   ├── cli.rs                  # new integration tests per command
│   └── fixtures/
│       └── stub-email-cli.sh   # mock contact update/delete responses
```

Each file has one responsibility. The `src/segment/` directory owns the filter expression language end-to-end; nothing else in the crate should understand pest or SQL-fragment generation.

---

## Design decisions locked before Task 1

Read these before touching any code. They explain choices that would otherwise feel arbitrary while executing steps. These reflect critical feedback from parallel Codex + Gemini reviews of an earlier draft — each decision here is deliberately narrowed.

1. **Filter precedence: `OR` lower than `AND` lower than `NOT`.** Matches every human language filter DSL the user has ever seen. `a OR b AND c` parses as `a OR (b AND c)`. Parens override.
2. **Filter literals are always bound as parameters** via `rusqlite::types::Value`. Table/column names that come from the AST (never from user strings) are whitelisted at compile time — the compiler module is the only place SQL strings are constructed. No string interpolation of user input, ever.
3. **`contact erase` is DEFERRED to Phase 7.** The spec §9.2 requires a full GDPR flow (PII rewrite + audit log + `email-cli contact delete` cross-system sync + suppression insert), and Phase 7 owns suppression CRUD. A Phase 3 stub would create fake compliance surface area. This narrows Phase 3 scope from what the parity plan originally listed. **Parity plan amendment noted in Task 26.**
4. **`contact resubscribe` is DEFERRED to Phase 7.** Spec §8.2 requires the command to refuse unless the matching suppression entry was already removed, and `suppression rm` is Phase 7. A command with no legitimate happy path is dead surface. **Parity plan amendment noted in Task 26.**
5. **`contact import --double-opt-in` errors with exit 3 in Phase 3.** The flag is visible in `--help` and `agent-info` so the CLI surface doesn't drift, but any attempt to use it returns `AppError::BadInput { code: "double_opt_in_not_available", message: "--double-opt-in requires `optin start`/`verify` which ship in v0.1.3 (Phase 7)", suggestion: "rerun without --double-opt-in; for now imported contacts default to status=active" }`. Real DOI lands in Phase 7.
6. **`contact set <email> <field> <value>` is LOCAL ONLY.** No sync-to-resend. The existing `field` schema has no metadata to mark a field as "should mirror to Resend properties", so adding that plumbing would need a schema migration. Phase 3 writes to `contact_field_value` and nothing more. Phase 9+ can revisit if operators ask.
7. **Type coercion is fail-fast at the CLI boundary.** `number` → `f64::from_str`, `bool` → `true|false|yes|no|1|0`, `date` → `chrono::DateTime::parse_from_rfc3339`, `text`/`select` → literal. `select` additionally checks the value is in `options_json`. Coercion failure returns exit 3 with a specific error citing the field name, expected type, and received value. Agents get predictable errors and can self-correct.
8. **`contact import` resumability = idempotent replay.** No cursor file, no import_jobs table. `contact_upsert` is idempotent, `contact_add_to_list` uses `INSERT OR IGNORE`, `contact_tag_add` uses `INSERT OR IGNORE`, and `contact_field_upsert` uses `INSERT OR REPLACE`. Re-running the same CSV after a crash re-visits rows but produces no duplicates. The acceptance criterion is reworded: "safe to rerun the same file without duplicates or state drift" — stronger than the parity plan's vague "resumable on failure".
9. **`contact import` rate-limiting wraps the subprocess layer, not rows.** A 200ms sleep is added to every `email-cli` invocation via a `EmailCli::throttle()` method that sleeps until 200ms have passed since the last call. Counts API calls, not rows. If `email-cli` exits with code 4 (RateLimited), the importer back-offs exponentially (1s, 2s, 4s, cap 30s). The 5 req/sec budget is enforced by a single shared timestamp on the `EmailCli` struct.
10. **`contact import` enforces `consent_source` (spec §9.3).** Every row must have a non-empty `consent_source` column OR the CLI must be invoked with `--unsafe-no-consent`. With `--unsafe-no-consent`, every imported row is auto-tagged `imported_without_consent` and a warning prints to stderr. Without the flag, a missing `consent_source` column (or a row with empty value) returns exit 3. This is non-negotiable — enforced at row-validation time, before any DB writes.
11. **Email syntax validation is minimal.** Reuse the existing `is_valid_email` helper from Phase 2. No MX lookup, no TLD database. Deliverability verification is a Phase 7 deliverable (`dnscheck` + verifier integration).
12. **Engagement atom semantics are cross-list and cross-broadcast**: `opened_last:30d` means "the contact has any `email.opened` event in the `event` table with `received_at` in the last 30 days" — regardless of which broadcast it came from. This is the spec §6 interpretation and the only one that evaluates quickly.
13. **`never_opened`** means "no row in `event` with `type = 'email.opened'` and `contact_id = c.id`" — across all history.
14. **`status` atom values** are whitelisted to the enum in the schema: `pending|active|unsubscribed|bounced|complained|cleaned|erased`. Any other value is a parse error.
15. **`bounced` bare atom resolves the spec inconsistency.** Spec §6 shows `NOT bounced` as a bare atom but the grammar only defines `status:active`-style keyed atoms. The AST adds an explicit `Atom::Bounced` which compiles to `(c.status = 'bounced' OR EXISTS (SELECT 1 FROM suppression WHERE email = c.email AND reason IN ('hard_bounced','soft_bounced_repeat')))`. Documented in the grammar file.
16. **Segments store the filter as JSON** (`serde_json::to_string(&expr)`), not as the original text. This lets the spec evolve without breaking saved segments and makes the stored form canonical. The original text can be re-derived from the AST if needed (Phase 4+ job).
17. **`segment members` and `contact ls --filter` share the same code path.** Both call `segment::compiler::to_sql_where(&expr)` and execute the produced WHERE against the same base query. A parity test in Task 20 proves they return identical result sets on the same dataset.
18. **`contact ls` gets full spec §4.2 pagination: `--list`, `--filter`, `--limit`, `--cursor`.** Cursor is the last `contact.id` seen, with stable `ORDER BY c.id ASC`. Agents can page through arbitrary-sized results without missing rows. `--limit` defaults to 100, max 10000.
19. **`contact add` accepts `--field key=val`** (spec §4.2) as a repeatable flag. Each pair is parsed into `(key, value)` and passed through the same typed-coercion path as `contact set`. Unknown field keys (not in the `field` table) return exit 3.
20. **`email_cli::contact_create` handles the duplicate-contact path correctly.** Previously (v0.0.3) the method silently treated "already exists" errors as success, which lets mailing-list-cli lose track of segment membership. Task 21 fixes this: on "already exists", the method fetches the existing Resend contact id and calls `segment_contact_add` for every segment in the original call. The local DB already tracks list membership independently, so this is purely about keeping the Resend mirror honest.
21. **Engagement query performance is not optimized in Phase 3.** Gemini's suggestion to add a `contact.last_active_at` cached column via a trigger is a Phase 6 task — it depends on `event` rows actually existing (from the webhook listener Phase 6 ships) and on a schema migration that only makes sense once engagement flows through the system. Until then, the raw subquery against `event` is correct — just slow — but there are zero event rows in Phase 3, so the queries finish instantly.

---

## Task 1: Add filter-parser and CSV dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the three new dependencies**

Edit `Cargo.toml`, inserting `csv`, `pest`, and `pest_derive` into `[dependencies]` alphabetically:

```toml
[dependencies]
anyhow = "1.0"
chrono = { version = "0.4", features = ["clock", "serde"] }
clap = { version = "4.5", features = ["derive", "env"] }
csv = "1.4"
dirs = "6.0"
pest = "2.8"
pest_derive = "2.8"
rusqlite = { version = "0.37", features = ["bundled", "chrono"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
toml = "0.8"
```

- [ ] **Step 2: Verify the deps resolve**

Run: `cargo build`
Expected: Successful build. All three crates pulled from crates.io. No compile errors because nothing uses them yet.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat(deps): add pest, pest_derive, csv for phase 3"
```

---

## Task 2: Segment module skeleton + AST types

**Files:**
- Create: `src/segment/mod.rs`
- Create: `src/segment/ast.rs`
- Modify: `src/main.rs` (add `mod segment;`)

- [ ] **Step 1: Create the module root**

Create `src/segment/mod.rs`:

```rust
//! Filter expression language for `segment create --filter <expr>` and
//! `contact ls --filter <expr>`. See docs/specs §6 for the full grammar.
//!
//! The pipeline is:
//!
//!   text  -->  [parser]  -->  SegmentExpr AST  -->  [compiler]  -->  (SQL fragment, params)
//!                                   |
//!                                   '-->  serde_json  -->  segment.filter_json column
//!
//! Nothing outside this module should understand pest or SQL-fragment generation.

pub mod ast;
pub mod compiler;
pub mod parser;

pub use ast::{Atom, EngagementAtom, FieldOp, ListPredicate, SegmentExpr, TagPredicate};
pub use compiler::to_sql_where;
pub use parser::{ParseError, parse};
```

- [ ] **Step 2: Create the AST types**

Create `src/segment/ast.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Top-level filter expression. Serializes to JSON for storage in
/// `segment.filter_json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SegmentExpr {
    /// Logical OR of one or more children.
    Or { children: Vec<SegmentExpr> },
    /// Logical AND of one or more children.
    And { children: Vec<SegmentExpr> },
    /// Logical NOT of a single child.
    Not { child: Box<SegmentExpr> },
    /// A leaf predicate.
    Atom { atom: Atom },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Atom {
    /// `status:active`, `status:unsubscribed`, etc.
    Status { value: String },
    /// `first_name:Alice`, `age:>:30`, `city:~:ber`
    Field {
        key: String,
        op: FieldOp,
        value: String,
    },
    /// `tag:vip`, `has_tag:vip`, `no_tag:spammer`
    Tag { pred: TagPredicate },
    /// `list:newsletter`, `in_list:news`, `not_in_list:archived`
    List { pred: ListPredicate },
    /// Engagement-based atoms that query the `event` table.
    Engagement { atom: EngagementAtom },
    /// `bounced` bare keyword (= status:bounced OR in suppression).
    Bounced,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldOp {
    Eq,
    Ne,
    Like,    // ~
    NotLike, // !~
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TagPredicate {
    Has { name: String },
    NotHas { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListPredicate {
    In { name: String },
    NotIn { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngagementAtom {
    /// Opened any broadcast in the last N <unit> (d/h/w/m).
    OpenedLast { duration: Duration },
    /// Clicked any broadcast in the last N <unit>.
    ClickedLast { duration: Duration },
    /// Was sent any broadcast in the last N <unit>.
    SentLast { duration: Duration },
    /// No `email.opened` event ever recorded for this contact.
    NeverOpened,
    /// No open OR click event within the given duration.
    InactiveFor { duration: Duration },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Duration {
    pub value: u32,
    pub unit: DurationUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationUnit {
    Hours,
    Days,
    Weeks,
    Months,
}

impl Duration {
    /// Duration as a SQLite `datetime('now', '-N <unit>')` modifier string.
    pub fn as_sqlite_offset(&self) -> String {
        let unit = match self.unit {
            DurationUnit::Hours => "hours",
            DurationUnit::Days => "days",
            DurationUnit::Weeks => "days", // 7 × value
            DurationUnit::Months => "months",
        };
        let value = match self.unit {
            DurationUnit::Weeks => (self.value as i64) * 7,
            _ => self.value as i64,
        };
        format!("-{value} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_expr_round_trips_through_json() {
        let expr = SegmentExpr::And {
            children: vec![
                SegmentExpr::Atom {
                    atom: Atom::Tag {
                        pred: TagPredicate::Has {
                            name: "vip".into(),
                        },
                    },
                },
                SegmentExpr::Not {
                    child: Box::new(SegmentExpr::Atom {
                        atom: Atom::Engagement {
                            atom: EngagementAtom::NeverOpened,
                        },
                    }),
                },
            ],
        };
        let json = serde_json::to_string(&expr).unwrap();
        let back: SegmentExpr = serde_json::from_str(&json).unwrap();
        assert_eq!(expr, back);
    }

    #[test]
    fn duration_sqlite_offset_days() {
        let d = Duration {
            value: 30,
            unit: DurationUnit::Days,
        };
        assert_eq!(d.as_sqlite_offset(), "-30 days");
    }

    #[test]
    fn duration_sqlite_offset_weeks_multiplies() {
        let d = Duration {
            value: 2,
            unit: DurationUnit::Weeks,
        };
        assert_eq!(d.as_sqlite_offset(), "-14 days");
    }
}
```

- [ ] **Step 3: Create stub `parser.rs` and `compiler.rs` so `mod.rs` compiles**

Create `src/segment/parser.rs`:

```rust
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
```

Create `src/segment/compiler.rs`:

```rust
//! SegmentExpr → SQL WHERE fragment compiler.
//! Full implementation lands in Tasks 5 and 6.

use crate::segment::ast::SegmentExpr;
use rusqlite::types::Value as SqlValue;

/// Compile a SegmentExpr to a `(fragment, params)` pair. The fragment is a
/// complete boolean expression that can be substituted into
/// `SELECT ... FROM contact c WHERE <fragment>`. The returned params match
/// the `?` placeholders in the fragment in order.
pub fn to_sql_where(_expr: &SegmentExpr) -> (String, Vec<SqlValue>) {
    // Task 5 + 6 implement this.
    ("1 = 1".into(), vec![])
}
```

- [ ] **Step 4: Wire `segment` module into the crate**

Edit `src/main.rs`, add `mod segment;` alongside the existing module declarations (keep alphabetical order):

```rust
mod cli;
mod commands;
mod config;
mod db;
mod email_cli;
mod error;
mod models;
mod output;
mod paths;
mod segment;
```

- [ ] **Step 5: Run the AST unit tests**

Run: `cargo test segment::ast`
Expected: 3 passed (`segment_expr_round_trips_through_json`, `duration_sqlite_offset_days`, `duration_sqlite_offset_weeks_multiplies`).

- [ ] **Step 6: Verify the whole crate still builds**

Run: `cargo build && cargo clippy -- -D warnings`
Expected: Clean build and clean clippy (no warnings from the new module).

- [ ] **Step 7: Commit**

```bash
git add src/segment/ src/main.rs
git commit -m "feat(segment): AST module with SegmentExpr, Atom, Duration + serde round-trip"
```

---

## Task 3: Pest grammar file for filter expressions

**Files:**
- Create: `src/segment/grammar.pest`
- Modify: `src/segment/parser.rs` (add `Parser` derive + compile the grammar)

- [ ] **Step 1: Write the pest grammar**

Create `src/segment/grammar.pest`:

```pest
// Filter expression grammar for mailing-list-cli.
// See docs/specs/2026-04-07-mailing-list-cli-design.md §6 for the language reference.

WHITESPACE = _{ " " | "\t" | "\r" | "\n" }

// Top-level
expression = { SOI ~ or_expr ~ EOI }

// Precedence: OR < AND < NOT < atom
or_expr  =  { and_expr ~ (or_op ~ and_expr)* }
and_expr =  { not_expr ~ (and_op ~ not_expr)* }
not_expr =  { not_op? ~ term }

or_op  = @{ ^"OR" }
and_op = @{ ^"AND" }
not_op = @{ ^"NOT" }

term = _{ paren | atom }
paren = { "(" ~ or_expr ~ ")" }

// Atoms
atom = _{
      engagement_atom
    | tag_atom
    | list_atom
    | bounced_atom
    | keyed_atom
}

// Engagement (order matters — longest prefix wins in pest)
engagement_atom = {
      opened_last
    | clicked_last
    | sent_last
    | inactive_for
    | never_opened
}
opened_last   = { ^"opened_last"   ~ ":" ~ duration }
clicked_last  = { ^"clicked_last"  ~ ":" ~ duration }
sent_last     = { ^"sent_last"     ~ ":" ~ duration }
inactive_for  = { ^"inactive_for"  ~ ":" ~ duration }
never_opened  = { ^"never_opened" }

// Tags and lists — longest prefix first
tag_atom = {
      has_tag_atom
    | no_tag_atom
    | tag_short
}
tag_short    = { ^"tag"     ~ ":" ~ ident }
has_tag_atom = { ^"has_tag" ~ ":" ~ ident }
no_tag_atom  = { ^"no_tag"  ~ ":" ~ ident }

list_atom = {
      in_list_atom
    | not_in_list_atom
    | list_short
}
list_short       = { ^"list"        ~ ":" ~ ident }
in_list_atom     = { ^"in_list"     ~ ":" ~ ident }
not_in_list_atom = { ^"not_in_list" ~ ":" ~ ident }

// Bare `bounced` keyword
bounced_atom = { ^"bounced" ~ !(":" | ident_char) }

// Generic `key:value` or `key:op:value` — the key must not be a reserved prefix.
// Reserved prefixes are handled above; `status:` and arbitrary field keys fall through here.
keyed_atom = { ident ~ ":" ~ (op_prefixed_value | implicit_eq_value) }
op_prefixed_value   = { op_token ~ ":" ~ value }
implicit_eq_value   = { value }
op_token = @{ "!=" | "!~" | ">=" | "<=" | "=" | "~" | ">" | "<" }

// Duration
duration = @{ ASCII_DIGIT+ ~ duration_unit }
duration_unit = @{ "d" | "h" | "w" | "m" }

// Identifiers, values
ident = @{ ident_char ~ ident_char* }
ident_char = _{ ASCII_ALPHANUMERIC | "_" | "-" | "." }

value = _{ quoted_value | bare_value }
quoted_value = @{ "\"" ~ (!"\"" ~ ANY)* ~ "\"" }
bare_value   = @{ (ASCII_ALPHANUMERIC | "_" | "-" | "." | "@" | "+")+ }
```

- [ ] **Step 2: Wire the grammar into the parser module**

Replace `src/segment/parser.rs` with the full parser skeleton. (The full parse logic lands in Task 4 — this step only gets the `Parser` derive compiling.)

```rust
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
```

- [ ] **Step 3: Run the grammar tests**

Run: `cargo test segment::parser`
Expected: 3 passed. The `grammar_*` tests confirm pest accepts / rejects correctly.

- [ ] **Step 4: Commit**

```bash
git add src/segment/grammar.pest src/segment/parser.rs
git commit -m "feat(segment): pest grammar for filter expressions"
```

---

## Task 4: Parser — walk pest pairs into SegmentExpr

**Files:**
- Modify: `src/segment/parser.rs`

- [ ] **Step 1: Implement the AST walker**

Replace `src/segment/parser.rs` with the full walker. This is the single longest code block in the phase — take the time to get it right.

```rust
//! Filter expression parser. Built on pest.

use crate::segment::ast::{
    Atom, Duration, DurationUnit, EngagementAtom, FieldOp, ListPredicate, SegmentExpr,
    TagPredicate,
};
use pest::Parser;
use pest::iterators::{Pair, Pairs};
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

/// Allowed values for the `status:` atom. Any other value is a parse error.
const STATUS_VALUES: &[&str] = &[
    "pending",
    "active",
    "unsubscribed",
    "bounced",
    "complained",
    "cleaned",
    "erased",
];

pub fn parse(input: &str) -> Result<SegmentExpr, ParseError> {
    let mut pairs = FilterParser::parse(Rule::expression, input).map_err(|e| {
        ParseError::new(
            format!("invalid filter expression: {e}"),
            "Check the grammar reference in the spec §6".to_string(),
        )
    })?;

    let expression = pairs
        .next()
        .ok_or_else(|| ParseError::new("empty input", "provide a filter expression"))?;
    // First child inside `expression` is the or_expr; second is EOI.
    let or_expr = expression
        .into_inner()
        .next()
        .ok_or_else(|| ParseError::new("empty or_expr", "provide a non-empty expression"))?;
    build_or(or_expr)
}

fn build_or(pair: Pair<Rule>) -> Result<SegmentExpr, ParseError> {
    // or_expr = and_expr (or_op and_expr)*
    let mut iter = pair.into_inner();
    let first = iter
        .next()
        .ok_or_else(|| ParseError::new("or_expr had no children", "internal parser bug"))?;
    let mut children = vec![build_and(first)?];
    while let Some(next) = iter.next() {
        match next.as_rule() {
            Rule::or_op => {}
            Rule::and_expr => children.push(build_and(next)?),
            other => {
                return Err(ParseError::new(
                    format!("unexpected rule in or_expr: {other:?}"),
                    "internal parser bug",
                ));
            }
        }
    }
    Ok(if children.len() == 1 {
        children.pop().unwrap()
    } else {
        SegmentExpr::Or { children }
    })
}

fn build_and(pair: Pair<Rule>) -> Result<SegmentExpr, ParseError> {
    let mut iter = pair.into_inner();
    let first = iter
        .next()
        .ok_or_else(|| ParseError::new("and_expr had no children", "internal parser bug"))?;
    let mut children = vec![build_not(first)?];
    while let Some(next) = iter.next() {
        match next.as_rule() {
            Rule::and_op => {}
            Rule::not_expr => children.push(build_not(next)?),
            other => {
                return Err(ParseError::new(
                    format!("unexpected rule in and_expr: {other:?}"),
                    "internal parser bug",
                ));
            }
        }
    }
    Ok(if children.len() == 1 {
        children.pop().unwrap()
    } else {
        SegmentExpr::And { children }
    })
}

fn build_not(pair: Pair<Rule>) -> Result<SegmentExpr, ParseError> {
    let mut iter = pair.into_inner();
    let first = iter
        .next()
        .ok_or_else(|| ParseError::new("not_expr had no children", "internal parser bug"))?;
    match first.as_rule() {
        Rule::not_op => {
            let inner = iter.next().ok_or_else(|| {
                ParseError::new("NOT without operand", "NOT must be followed by a term")
            })?;
            let child = build_term(inner)?;
            Ok(SegmentExpr::Not {
                child: Box::new(child),
            })
        }
        _ => build_term(first),
    }
}

fn build_term(pair: Pair<Rule>) -> Result<SegmentExpr, ParseError> {
    match pair.as_rule() {
        Rule::paren => {
            let inner = pair
                .into_inner()
                .next()
                .ok_or_else(|| ParseError::new("empty parens", "parens cannot be empty"))?;
            build_or(inner)
        }
        Rule::engagement_atom => Ok(SegmentExpr::Atom {
            atom: Atom::Engagement {
                atom: build_engagement(pair)?,
            },
        }),
        Rule::tag_atom => Ok(SegmentExpr::Atom {
            atom: Atom::Tag {
                pred: build_tag(pair)?,
            },
        }),
        Rule::list_atom => Ok(SegmentExpr::Atom {
            atom: Atom::List {
                pred: build_list(pair)?,
            },
        }),
        Rule::bounced_atom => Ok(SegmentExpr::Atom { atom: Atom::Bounced }),
        Rule::keyed_atom => build_keyed(pair),
        other => Err(ParseError::new(
            format!("unexpected term rule: {other:?}"),
            "internal parser bug",
        )),
    }
}

fn build_engagement(pair: Pair<Rule>) -> Result<EngagementAtom, ParseError> {
    let inner = pair
        .into_inner()
        .next()
        .ok_or_else(|| ParseError::new("empty engagement atom", "internal parser bug"))?;
    match inner.as_rule() {
        Rule::opened_last => {
            let dur = pair_last_duration(inner)?;
            Ok(EngagementAtom::OpenedLast { duration: dur })
        }
        Rule::clicked_last => {
            let dur = pair_last_duration(inner)?;
            Ok(EngagementAtom::ClickedLast { duration: dur })
        }
        Rule::sent_last => {
            let dur = pair_last_duration(inner)?;
            Ok(EngagementAtom::SentLast { duration: dur })
        }
        Rule::inactive_for => {
            let dur = pair_last_duration(inner)?;
            Ok(EngagementAtom::InactiveFor { duration: dur })
        }
        Rule::never_opened => Ok(EngagementAtom::NeverOpened),
        other => Err(ParseError::new(
            format!("unexpected engagement rule: {other:?}"),
            "internal parser bug",
        )),
    }
}

fn pair_last_duration(pair: Pair<Rule>) -> Result<Duration, ParseError> {
    let duration_pair = pair
        .into_inner()
        .find(|p| p.as_rule() == Rule::duration)
        .ok_or_else(|| ParseError::new("missing duration", "expected e.g. `30d`, `6h`, `2w`, `3m`"))?;
    parse_duration(duration_pair.as_str())
}

fn parse_duration(s: &str) -> Result<Duration, ParseError> {
    if s.len() < 2 {
        return Err(ParseError::new(
            format!("invalid duration '{s}'"),
            "use a number followed by d/h/w/m, e.g. `30d`",
        ));
    }
    let (num_part, unit_part) = s.split_at(s.len() - 1);
    let value: u32 = num_part.parse().map_err(|_| {
        ParseError::new(
            format!("invalid duration number '{num_part}'"),
            "duration must be a positive integer",
        )
    })?;
    let unit = match unit_part {
        "d" => DurationUnit::Days,
        "h" => DurationUnit::Hours,
        "w" => DurationUnit::Weeks,
        "m" => DurationUnit::Months,
        other => {
            return Err(ParseError::new(
                format!("invalid duration unit '{other}'"),
                "use d (days), h (hours), w (weeks), or m (months)",
            ));
        }
    };
    Ok(Duration { value, unit })
}

fn build_tag(pair: Pair<Rule>) -> Result<TagPredicate, ParseError> {
    let inner = pair
        .into_inner()
        .next()
        .ok_or_else(|| ParseError::new("empty tag atom", "internal parser bug"))?;
    let name = extract_ident(&inner)?;
    match inner.as_rule() {
        Rule::tag_short | Rule::has_tag_atom => Ok(TagPredicate::Has { name }),
        Rule::no_tag_atom => Ok(TagPredicate::NotHas { name }),
        other => Err(ParseError::new(
            format!("unexpected tag rule: {other:?}"),
            "internal parser bug",
        )),
    }
}

fn build_list(pair: Pair<Rule>) -> Result<ListPredicate, ParseError> {
    let inner = pair
        .into_inner()
        .next()
        .ok_or_else(|| ParseError::new("empty list atom", "internal parser bug"))?;
    let name = extract_ident(&inner)?;
    match inner.as_rule() {
        Rule::list_short | Rule::in_list_atom => Ok(ListPredicate::In { name }),
        Rule::not_in_list_atom => Ok(ListPredicate::NotIn { name }),
        other => Err(ParseError::new(
            format!("unexpected list rule: {other:?}"),
            "internal parser bug",
        )),
    }
}

fn extract_ident(pair: &Pair<Rule>) -> Result<String, ParseError> {
    let ident = pair
        .clone()
        .into_inner()
        .find(|p| p.as_rule() == Rule::ident)
        .ok_or_else(|| ParseError::new("missing identifier", "expected a name after ':'"))?;
    Ok(ident.as_str().to_string())
}

fn build_keyed(pair: Pair<Rule>) -> Result<SegmentExpr, ParseError> {
    let mut iter = pair.into_inner();
    let key_pair = iter
        .next()
        .ok_or_else(|| ParseError::new("keyed atom missing key", "internal parser bug"))?;
    let key = key_pair.as_str().to_string();

    let value_pair = iter
        .next()
        .ok_or_else(|| ParseError::new("keyed atom missing value", "provide a value after ':'"))?;
    let (op, value) = parse_value_side(value_pair)?;

    if key == "status" {
        if op != FieldOp::Eq {
            return Err(ParseError::new(
                format!("status atom only supports '=' (got {op:?})"),
                "use `status:active`, `status:bounced`, etc.",
            ));
        }
        if !STATUS_VALUES.contains(&value.as_str()) {
            return Err(ParseError::new(
                format!("unknown status '{value}'"),
                format!("valid statuses: {}", STATUS_VALUES.join(", ")),
            ));
        }
        return Ok(SegmentExpr::Atom {
            atom: Atom::Status { value },
        });
    }

    Ok(SegmentExpr::Atom {
        atom: Atom::Field { key, op, value },
    })
}

fn parse_value_side(pair: Pair<Rule>) -> Result<(FieldOp, String), ParseError> {
    match pair.as_rule() {
        Rule::op_prefixed_value => {
            let mut iter = pair.into_inner();
            let op_tok = iter
                .next()
                .ok_or_else(|| ParseError::new("missing op token", "internal parser bug"))?;
            let op = parse_op(op_tok.as_str())?;
            let val = iter
                .next()
                .ok_or_else(|| ParseError::new("missing value after op", "expected a value"))?;
            Ok((op, strip_quotes(val.as_str())))
        }
        Rule::implicit_eq_value => {
            let val = pair
                .into_inner()
                .next()
                .ok_or_else(|| ParseError::new("missing value", "expected a value"))?;
            Ok((FieldOp::Eq, strip_quotes(val.as_str())))
        }
        other => Err(ParseError::new(
            format!("unexpected value rule: {other:?}"),
            "internal parser bug",
        )),
    }
}

fn parse_op(s: &str) -> Result<FieldOp, ParseError> {
    Ok(match s {
        "=" => FieldOp::Eq,
        "!=" => FieldOp::Ne,
        "~" => FieldOp::Like,
        "!~" => FieldOp::NotLike,
        ">" => FieldOp::Gt,
        ">=" => FieldOp::Ge,
        "<" => FieldOp::Lt,
        "<=" => FieldOp::Le,
        other => {
            return Err(ParseError::new(
                format!("unknown operator '{other}'"),
                "use one of: = != ~ !~ > >= < <=",
            ));
        }
    })
}

fn strip_quotes(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[allow(dead_code)]
fn debug_pairs(pairs: Pairs<Rule>) {
    for p in pairs {
        eprintln!("{:?} = '{}'", p.as_rule(), p.as_str());
        debug_pairs(p.into_inner());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::ast::{Atom, DurationUnit, EngagementAtom};

    fn atom(a: Atom) -> SegmentExpr {
        SegmentExpr::Atom { atom: a }
    }

    #[test]
    fn parses_bare_tag() {
        let e = parse("tag:vip").unwrap();
        assert_eq!(
            e,
            atom(Atom::Tag {
                pred: TagPredicate::Has { name: "vip".into() }
            })
        );
    }

    #[test]
    fn parses_and_of_tag_and_engagement() {
        let e = parse("tag:vip AND opened_last:30d").unwrap();
        match e {
            SegmentExpr::And { children } => {
                assert_eq!(children.len(), 2);
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parses_list_and_not_bounced() {
        let e = parse("list:newsletter AND NOT bounced").unwrap();
        match e {
            SegmentExpr::And { children } => {
                assert_eq!(children.len(), 2);
                assert!(matches!(
                    &children[1],
                    SegmentExpr::Not { .. }
                ));
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parses_status_field_and_engagement() {
        let e = parse("status:active AND city:Berlin AND opened_last:90d").unwrap();
        match e {
            SegmentExpr::And { children } => assert_eq!(children.len(), 3),
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parses_grouped_or_inside_and() {
        let e = parse("has_tag:premium AND (clicked_last:7d OR opened_last:14d)").unwrap();
        match e {
            SegmentExpr::And { children } => {
                assert_eq!(children.len(), 2);
                assert!(matches!(&children[1], SegmentExpr::Or { .. }));
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parses_inactive_for_with_not_has_tag() {
        let e = parse("inactive_for:180d AND NOT has_tag:do_not_sunset").unwrap();
        match e {
            SegmentExpr::And { children } => {
                assert_eq!(children.len(), 2);
                assert!(matches!(
                    &children[0],
                    SegmentExpr::Atom {
                        atom: Atom::Engagement {
                            atom: EngagementAtom::InactiveFor { .. }
                        }
                    }
                ));
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parses_like_operator() {
        let e = parse("first_name:~:ali").unwrap();
        assert_eq!(
            e,
            atom(Atom::Field {
                key: "first_name".into(),
                op: FieldOp::Like,
                value: "ali".into()
            })
        );
    }

    #[test]
    fn parses_greater_than() {
        let e = parse("age:>:30").unwrap();
        assert_eq!(
            e,
            atom(Atom::Field {
                key: "age".into(),
                op: FieldOp::Gt,
                value: "30".into()
            })
        );
    }

    #[test]
    fn rejects_unknown_status() {
        let err = parse("status:confused").unwrap_err();
        assert!(err.message.contains("unknown status"));
    }

    #[test]
    fn rejects_invalid_duration_unit() {
        assert!(parse("opened_last:30x").is_err());
    }

    #[test]
    fn duration_weeks_parses() {
        let e = parse("opened_last:2w").unwrap();
        match e {
            SegmentExpr::Atom {
                atom:
                    Atom::Engagement {
                        atom: EngagementAtom::OpenedLast { duration },
                    },
            } => {
                assert_eq!(duration.value, 2);
                assert_eq!(duration.unit, DurationUnit::Weeks);
            }
            other => panic!("expected OpenedLast, got {other:?}"),
        }
    }

    #[test]
    fn parsed_expression_round_trips_through_json() {
        let src = "has_tag:premium AND (clicked_last:7d OR opened_last:14d)";
        let expr = parse(src).unwrap();
        let json = serde_json::to_string(&expr).unwrap();
        let back: SegmentExpr = serde_json::from_str(&json).unwrap();
        assert_eq!(expr, back);
    }
}
```

- [ ] **Step 2: Run the full parser test suite**

Run: `cargo test segment::parser`
Expected: All 12 tests pass (3 grammar tests from Task 3 + 9 new walker tests).

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 4: Commit**

```bash
git add src/segment/parser.rs
git commit -m "feat(segment): walk pest pairs into SegmentExpr AST"
```

---

## Task 5: SQL compiler — simple atoms

**Files:**
- Modify: `src/segment/compiler.rs`

- [ ] **Step 1: Implement the compiler for non-engagement atoms**

Replace `src/segment/compiler.rs` with:

```rust
//! SegmentExpr → SQL WHERE fragment compiler.
//!
//! Contract: the returned fragment is a complete boolean expression that can
//! be substituted into `SELECT ... FROM contact c WHERE <fragment>`. All
//! literal values from the AST flow through rusqlite parameters as `?`
//! placeholders in positional order. The compiler is the only module in the
//! crate that constructs SQL strings involving user input.

use crate::segment::ast::{
    Atom, EngagementAtom, FieldOp, ListPredicate, SegmentExpr, TagPredicate,
};
use rusqlite::types::Value as SqlValue;

/// Compile the expression. Returns `(fragment, params)`.
pub fn to_sql_where(expr: &SegmentExpr) -> (String, Vec<SqlValue>) {
    let mut ctx = Ctx::default();
    let sql = compile(expr, &mut ctx);
    (sql, ctx.params)
}

#[derive(Default)]
struct Ctx {
    params: Vec<SqlValue>,
}

impl Ctx {
    fn push(&mut self, v: SqlValue) -> &'static str {
        self.params.push(v);
        "?"
    }
}

fn compile(expr: &SegmentExpr, ctx: &mut Ctx) -> String {
    match expr {
        SegmentExpr::Or { children } => {
            if children.is_empty() {
                return "0".to_string();
            }
            let parts: Vec<String> = children.iter().map(|c| compile(c, ctx)).collect();
            format!("({})", parts.join(" OR "))
        }
        SegmentExpr::And { children } => {
            if children.is_empty() {
                return "1".to_string();
            }
            let parts: Vec<String> = children.iter().map(|c| compile(c, ctx)).collect();
            format!("({})", parts.join(" AND "))
        }
        SegmentExpr::Not { child } => {
            let inner = compile(child, ctx);
            format!("(NOT {inner})")
        }
        SegmentExpr::Atom { atom } => compile_atom(atom, ctx),
    }
}

fn compile_atom(atom: &Atom, ctx: &mut Ctx) -> String {
    match atom {
        Atom::Status { value } => {
            let p = ctx.push(SqlValue::Text(value.clone()));
            format!("c.status = {p}")
        }
        Atom::Bounced => {
            // `bounced` bare keyword: contact is bounced OR on suppression for hard bounce.
            let hard = ctx.push(SqlValue::Text("hard_bounced".into()));
            let soft = ctx.push(SqlValue::Text("soft_bounced_repeat".into()));
            format!(
                "(c.status = 'bounced' OR EXISTS (SELECT 1 FROM suppression s WHERE s.email = c.email AND s.reason IN ({hard}, {soft})))"
            )
        }
        Atom::Field { key, op, value } => compile_field(key, *op, value, ctx),
        Atom::Tag { pred } => compile_tag(pred, ctx),
        Atom::List { pred } => compile_list(pred, ctx),
        Atom::Engagement { atom } => compile_engagement(atom, ctx),
    }
}

fn compile_field(key: &str, op: FieldOp, value: &str, ctx: &mut Ctx) -> String {
    // Tier 1: built-in contact columns (whitelisted — never user-interpolated).
    let builtin = match key {
        "email" => Some("c.email"),
        "first_name" => Some("c.first_name"),
        "last_name" => Some("c.last_name"),
        _ => None,
    };
    if let Some(col) = builtin {
        return format_op(col, op, value, ctx);
    }

    // Tier 2: custom field lookup via contact_field_value.
    // Value coercion: if the value parses as a number we bind as Real; if it
    // parses as "true"/"false" we bind as Integer 1/0; otherwise as Text.
    // The column chosen in the subquery mirrors the choice.
    let key_param = ctx.push(SqlValue::Text(key.to_string()));
    let (col, sql_val) = coerce_value(value);
    let value_param = ctx.push(sql_val);
    let op_sql = op_to_sql(op);
    let like_wrap = matches!(op, FieldOp::Like | FieldOp::NotLike);
    let value_expr = if like_wrap {
        format!("'%' || {value_param} || '%'")
    } else {
        value_param.to_string()
    };
    format!(
        "c.id IN (SELECT cfv.contact_id FROM contact_field_value cfv \
         JOIN field f ON cfv.field_id = f.id \
         WHERE f.key = {key_param} AND cfv.{col} {op_sql} {value_expr})"
    )
}

fn format_op(col: &str, op: FieldOp, value: &str, ctx: &mut Ctx) -> String {
    let (col_expr, bind_val) = if matches!(op, FieldOp::Like | FieldOp::NotLike) {
        (col.to_string(), SqlValue::Text(format!("%{value}%")))
    } else {
        (col.to_string(), SqlValue::Text(value.to_string()))
    };
    let p = ctx.push(bind_val);
    let op_sql = op_to_sql(op);
    format!("{col_expr} {op_sql} {p}")
}

fn op_to_sql(op: FieldOp) -> &'static str {
    match op {
        FieldOp::Eq => "=",
        FieldOp::Ne => "!=",
        FieldOp::Like => "LIKE",
        FieldOp::NotLike => "NOT LIKE",
        FieldOp::Gt => ">",
        FieldOp::Ge => ">=",
        FieldOp::Lt => "<",
        FieldOp::Le => "<=",
    }
}

/// Pick the best column in `contact_field_value` for the given literal.
fn coerce_value(value: &str) -> (&'static str, SqlValue) {
    if let Ok(n) = value.parse::<f64>() {
        return ("value_number", SqlValue::Real(n));
    }
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => return ("value_bool", SqlValue::Integer(1)),
        "false" | "no" | "0" => return ("value_bool", SqlValue::Integer(0)),
        _ => {}
    }
    ("value_text", SqlValue::Text(value.to_string()))
}

fn compile_tag(pred: &TagPredicate, ctx: &mut Ctx) -> String {
    let (name, negate) = match pred {
        TagPredicate::Has { name } => (name.clone(), false),
        TagPredicate::NotHas { name } => (name.clone(), true),
    };
    let p = ctx.push(SqlValue::Text(name));
    let subq = format!(
        "c.id IN (SELECT ct.contact_id FROM contact_tag ct JOIN tag t ON ct.tag_id = t.id WHERE t.name = {p})"
    );
    if negate {
        format!("(NOT {subq})")
    } else {
        subq
    }
}

fn compile_list(pred: &ListPredicate, ctx: &mut Ctx) -> String {
    let (name, negate) = match pred {
        ListPredicate::In { name } => (name.clone(), false),
        ListPredicate::NotIn { name } => (name.clone(), true),
    };
    let p = ctx.push(SqlValue::Text(name));
    let subq = format!(
        "c.id IN (SELECT lm.contact_id FROM list_membership lm JOIN list l ON lm.list_id = l.id WHERE l.name = {p})"
    );
    if negate {
        format!("(NOT {subq})")
    } else {
        subq
    }
}

fn compile_engagement(atom: &EngagementAtom, ctx: &mut Ctx) -> String {
    match atom {
        EngagementAtom::OpenedLast { duration } => {
            let p = ctx.push(SqlValue::Text(duration.as_sqlite_offset()));
            format!(
                "c.id IN (SELECT e.contact_id FROM event e \
                 WHERE e.type = 'email.opened' \
                 AND e.received_at >= datetime('now', {p}))"
            )
        }
        EngagementAtom::ClickedLast { duration } => {
            let p = ctx.push(SqlValue::Text(duration.as_sqlite_offset()));
            format!(
                "c.id IN (SELECT e.contact_id FROM event e \
                 WHERE e.type = 'email.clicked' \
                 AND e.received_at >= datetime('now', {p}))"
            )
        }
        EngagementAtom::SentLast { duration } => {
            let p = ctx.push(SqlValue::Text(duration.as_sqlite_offset()));
            format!(
                "c.id IN (SELECT br.contact_id FROM broadcast_recipient br \
                 WHERE br.sent_at >= datetime('now', {p}))"
            )
        }
        EngagementAtom::NeverOpened => "c.id NOT IN (SELECT e.contact_id FROM event e WHERE e.type = 'email.opened' AND e.contact_id IS NOT NULL)".to_string(),
        EngagementAtom::InactiveFor { duration } => {
            let p = ctx.push(SqlValue::Text(duration.as_sqlite_offset()));
            format!(
                "c.id NOT IN (SELECT e.contact_id FROM event e \
                 WHERE e.type IN ('email.opened', 'email.clicked') \
                 AND e.received_at >= datetime('now', {p}))"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::parser::parse;

    #[test]
    fn compiles_simple_tag_to_subquery() {
        let expr = parse("tag:vip").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert!(sql.contains("contact_tag"));
        assert!(sql.contains("t.name = ?"));
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], SqlValue::Text("vip".into()));
    }

    #[test]
    fn compiles_and_of_tag_and_engagement() {
        let expr = parse("tag:vip AND opened_last:30d").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert!(sql.starts_with('('));
        assert!(sql.contains(" AND "));
        assert!(sql.contains("contact_tag"));
        assert!(sql.contains("event"));
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn compiles_mixed_and_or_not() {
        let expr = parse("has_tag:premium AND (clicked_last:7d OR opened_last:14d)").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert!(sql.contains(" OR "));
        assert!(sql.contains(" AND "));
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn compiles_status_directly() {
        let expr = parse("status:active").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert_eq!(sql, "c.status = ?");
        assert_eq!(params, vec![SqlValue::Text("active".into())]);
    }

    #[test]
    fn compiles_builtin_first_name_like() {
        let expr = parse("first_name:~:ali").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert!(sql.contains("c.first_name LIKE"));
        assert_eq!(params, vec![SqlValue::Text("%ali%".into())]);
    }

    #[test]
    fn compiles_custom_field_number() {
        let expr = parse("age:>:30").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert!(sql.contains("contact_field_value"));
        assert!(sql.contains("value_number >"));
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], SqlValue::Text("age".into()));
        assert_eq!(params[1], SqlValue::Real(30.0));
    }

    #[test]
    fn compiles_never_opened_without_params() {
        let expr = parse("never_opened").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert!(sql.contains("NOT IN"));
        assert!(sql.contains("email.opened"));
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn compiles_list_in_with_name_param() {
        let expr = parse("list:newsletter").unwrap();
        let (sql, params) = to_sql_where(&expr);
        assert!(sql.contains("list_membership"));
        assert_eq!(params, vec![SqlValue::Text("newsletter".into())]);
    }

    #[test]
    fn compiles_not_wraps_with_paren() {
        let expr = parse("NOT tag:vip").unwrap();
        let (sql, _) = to_sql_where(&expr);
        assert!(sql.starts_with("(NOT "));
    }
}
```

- [ ] **Step 2: Run the compiler test suite**

Run: `cargo test segment::compiler`
Expected: 9 passed.

- [ ] **Step 3: Run the full segment module suite to confirm no regressions**

Run: `cargo test segment::`
Expected: All AST + parser + compiler tests pass (~24 total).

- [ ] **Step 4: Commit**

```bash
git add src/segment/compiler.rs
git commit -m "feat(segment): SQL WHERE compiler with parameterized bindings"
```

---

## Task 6: Smoke-test the compiled WHERE against a real SQLite

**Files:**
- Modify: `src/segment/compiler.rs` (add a SQL execution test)

- [ ] **Step 1: Add an integration test that actually runs the compiled SQL**

Append to the `tests` module in `src/segment/compiler.rs`, above the closing `}`:

```rust
    #[test]
    fn compiled_sql_executes_against_sqlite() {
        use crate::db::Db;
        use rusqlite::params_from_iter;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        // Seed one list, one contact, one tag
        let list_id = db.list_create("news", None, "aud_x").unwrap();
        let alice = db
            .contact_upsert("alice@example.com", Some("Alice"), None)
            .unwrap();
        db.contact_add_to_list(alice, list_id).unwrap();
        db.conn
            .execute(
                "INSERT INTO tag (name) VALUES ('vip')",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO contact_tag (contact_id, tag_id, applied_at) \
                 VALUES (?, (SELECT id FROM tag WHERE name='vip'), datetime('now'))",
                [alice],
            )
            .unwrap();

        let expr = parse("tag:vip").unwrap();
        let (frag, params) = to_sql_where(&expr);
        let sql = format!("SELECT c.id FROM contact c WHERE {frag}");
        let mut stmt = db.conn.prepare(&sql).unwrap();
        let rows: Vec<i64> = stmt
            .query_map(params_from_iter(params.iter()), |r| r.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![alice]);
    }
```

Add `tempfile` to `[dependencies]` in `Cargo.toml` if not already present as a dev-dep (it already is in `[dev-dependencies]`, so `#[cfg(test)]` code can use it — no change needed).

- [ ] **Step 2: Run the new test**

Run: `cargo test segment::compiler::tests::compiled_sql_executes_against_sqlite`
Expected: 1 passed.

- [ ] **Step 3: Commit**

```bash
git add src/segment/compiler.rs
git commit -m "test(segment): round-trip compiled SQL through real SQLite"
```

---

## Task 7: DB helpers for tags

**Files:**
- Modify: `src/db/mod.rs`
- Modify: `src/models.rs`

- [ ] **Step 1: Add the `Tag` model struct**

Append to `src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub member_count: i64,
}
```

- [ ] **Step 2: Add tag + contact_tag DB helpers**

Add the following to `impl Db` in `src/db/mod.rs`, after the contact helpers:

```rust
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
        match self.conn.query_row(
            "SELECT id FROM tag WHERE name = ?1",
            params![name],
            |r| r.get::<_, i64>(0),
        ) {
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
```

- [ ] **Step 3: Add unit tests for tag helpers**

Append to the `tests` module at the bottom of `src/db/mod.rs`, before the closing `}`:

```rust
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
        let contact = db
            .contact_upsert("alice@example.com", None, None)
            .unwrap();
        let tag = db.tag_get_or_create("vip").unwrap();
        db.contact_tag_add(contact, tag).unwrap();
        assert_eq!(db.contact_tags_for(contact).unwrap(), vec!["vip".to_string()]);
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
        let contact = db
            .contact_upsert("alice@example.com", None, None)
            .unwrap();
        let tag_id = db.tag_get_or_create("vip").unwrap();
        db.contact_tag_add(contact, tag_id).unwrap();
        assert!(db.tag_delete("vip").unwrap());
        assert!(db.contact_tags_for(contact).unwrap().is_empty());
    }
```

- [ ] **Step 4: Run the new DB tests**

Run: `cargo test db::tests::tag`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/db/mod.rs src/models.rs
git commit -m "feat(db): tag_* and contact_tag_* helpers"
```

---

## Task 8: `tag ls` and `tag rm` CLI commands

**Files:**
- Create: `src/commands/tag.rs`
- Modify: `src/commands/mod.rs` (add `pub mod tag;`)
- Modify: `src/cli.rs` (add Tag subcommand)
- Modify: `src/main.rs` (dispatch Tag)

- [ ] **Step 1: Add the CLI definitions**

Append to `src/cli.rs` (add the variant inside `enum Command`, and add the new action enum at the bottom):

Insert into the `Command` enum after the `Contact` variant:

```rust
    /// Manage tags (n:m with contacts)
    Tag {
        #[command(subcommand)]
        action: TagAction,
    },
```

Add at the bottom of the file:

```rust
#[derive(Subcommand, Debug)]
pub enum TagAction {
    /// List all tags with member counts
    #[command(visible_alias = "ls")]
    List,
    /// Delete a tag (removes from all contacts)
    Rm(TagRmArgs),
}

#[derive(Args, Debug)]
pub struct TagRmArgs {
    /// Tag name
    pub name: String,
    /// Explicit confirmation (required)
    #[arg(long)]
    pub confirm: bool,
}
```

- [ ] **Step 2: Create the tag command module**

Create `src/commands/tag.rs`:

```rust
use crate::cli::{TagAction, TagRmArgs};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: TagAction) -> Result<(), AppError> {
    let db = Db::open()?;
    match action {
        TagAction::List => list_tags(format, &db),
        TagAction::Rm(args) => remove_tag(format, &db, args),
    }
}

fn list_tags(format: Format, db: &Db) -> Result<(), AppError> {
    let tags = db.tag_all()?;
    let count = tags.len();
    output::success(
        format,
        &format!("{count} tag(s)"),
        json!({ "tags": tags, "count": count }),
    );
    Ok(())
}

fn remove_tag(format: Format, db: &Db, args: TagRmArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("deleting tag '{}' requires --confirm", args.name),
            suggestion: format!("rerun with `mailing-list-cli tag rm {} --confirm`", args.name),
        });
    }
    let removed = db.tag_delete(&args.name)?;
    if !removed {
        return Err(AppError::BadInput {
            code: "tag_not_found".into(),
            message: format!("no tag named '{}'", args.name),
            suggestion: "Run `mailing-list-cli tag ls` to see all tags".into(),
        });
    }
    output::success(
        format,
        &format!("tag '{}' removed", args.name),
        json!({ "name": args.name, "removed": true }),
    );
    Ok(())
}
```

- [ ] **Step 3: Wire the tag module**

Edit `src/commands/mod.rs` to include:

```rust
pub mod agent_info;
pub mod contact;
pub mod field;
pub mod health;
pub mod list;
pub mod segment;
pub mod skill;
pub mod tag;
pub mod update;
```

(Add `field`, `segment`, and `tag` now so we don't revisit this file in every later task. The other modules get created in subsequent tasks.)

Edit `src/main.rs` to dispatch the Tag command. Add to the match:

```rust
        Command::Tag { action } => commands::tag::run(format, action),
```

Then **add empty placeholder files so the crate compiles** (we'll implement them in the next tasks):

Create `src/commands/field.rs`:

```rust
use crate::cli::FieldAction;
use crate::error::AppError;
use crate::output::Format;

pub fn run(_format: Format, _action: FieldAction) -> Result<(), AppError> {
    Err(AppError::BadInput {
        code: "not_implemented".into(),
        message: "field commands not yet implemented".into(),
        suggestion: "implement in Task 10".into(),
    })
}
```

Create `src/commands/segment.rs`:

```rust
use crate::cli::SegmentAction;
use crate::error::AppError;
use crate::output::Format;

pub fn run(_format: Format, _action: SegmentAction) -> Result<(), AppError> {
    Err(AppError::BadInput {
        code: "not_implemented".into(),
        message: "segment commands not yet implemented".into(),
        suggestion: "implement in Task 19".into(),
    })
}
```

And **add the `Field` and `Segment` CLI stubs** to `src/cli.rs` so the module compiles. Add inside `enum Command` (after `Tag`):

```rust
    /// Manage custom fields
    Field {
        #[command(subcommand)]
        action: FieldAction,
    },
    /// Manage dynamic segments (saved filters)
    Segment {
        #[command(subcommand)]
        action: SegmentAction,
    },
```

Add at the bottom of `src/cli.rs`:

```rust
#[derive(Subcommand, Debug)]
pub enum FieldAction {
    /// Placeholder — full impl in Task 10
    #[command(visible_alias = "ls")]
    List,
}

#[derive(Subcommand, Debug)]
pub enum SegmentAction {
    /// Placeholder — full impl in Task 19
    #[command(visible_alias = "ls")]
    List,
}
```

Add dispatch for Field and Segment in `src/main.rs`:

```rust
        Command::Field { action } => commands::field::run(format, action),
        Command::Segment { action } => commands::segment::run(format, action),
```

- [ ] **Step 4: Verify the crate builds**

Run: `cargo build`
Expected: Clean build.

- [ ] **Step 5: Add an integration test for `tag ls`**

Append to `tests/cli.rs`:

```rust
#[test]
fn tag_ls_on_empty_db_returns_count_zero() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "ls"]);
    let out = cmd.assert().success();
    let value: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(value["data"]["count"], 0);
}

#[test]
fn tag_rm_without_confirm_fails() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "rm", "vip"]);
    cmd.assert().failure().code(3);
}
```

- [ ] **Step 6: Run the new tests + full sweep**

Run: `cargo test tag_ls_on_empty_db_returns_count_zero tag_rm_without_confirm_fails`
Expected: 2 passed.

Then run: `cargo test`
Expected: All prior tests still pass.

- [ ] **Step 7: Commit**

```bash
git add src/cli.rs src/commands/ src/main.rs tests/cli.rs
git commit -m "feat(tag): tag ls / tag rm commands + stub field/segment modules"
```

---

## Task 9: `contact tag` and `contact untag` CLI commands

**Files:**
- Modify: `src/cli.rs` (extend `ContactAction`)
- Modify: `src/commands/contact.rs` (dispatch + impl)

- [ ] **Step 1: Extend `ContactAction` enum**

In `src/cli.rs`, replace `enum ContactAction` with:

```rust
#[derive(Subcommand, Debug)]
pub enum ContactAction {
    /// Add a contact to a list (also writes through to the Resend segment)
    Add(ContactAddArgs),
    /// List contacts in a list
    #[command(visible_alias = "ls")]
    List(ContactListArgs),
    /// Apply a tag to a contact
    Tag(ContactTagArgs),
    /// Remove a tag from a contact
    Untag(ContactTagArgs),
}

#[derive(Args, Debug)]
pub struct ContactTagArgs {
    /// Contact email
    pub email: String,
    /// Tag name
    pub tag: String,
}
```

- [ ] **Step 2: Extend `src/commands/contact.rs`**

Update the `run` function and add `tag_contact` / `untag_contact` helpers:

```rust
pub fn run(format: Format, action: ContactAction) -> Result<(), AppError> {
    let config = Config::load()?;
    let db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    match action {
        ContactAction::Add(args) => add(format, &db, &cli, args),
        ContactAction::List(args) => list_contacts(format, &db, args),
        ContactAction::Tag(args) => tag_contact(format, &db, args),
        ContactAction::Untag(args) => untag_contact(format, &db, args),
    }
}
```

Add at the bottom of the file, before the `tests` module:

```rust
fn tag_contact(
    format: Format,
    db: &Db,
    args: crate::cli::ContactTagArgs,
) -> Result<(), AppError> {
    let contact_id = contact_id_or_fail(db, &args.email)?;
    let tag_id = db.tag_get_or_create(&args.tag)?;
    db.contact_tag_add(contact_id, tag_id)?;
    output::success(
        format,
        &format!("tagged {} with '{}'", args.email, args.tag),
        json!({
            "email": args.email,
            "tag": args.tag,
            "contact_id": contact_id
        }),
    );
    Ok(())
}

fn untag_contact(
    format: Format,
    db: &Db,
    args: crate::cli::ContactTagArgs,
) -> Result<(), AppError> {
    let contact_id = contact_id_or_fail(db, &args.email)?;
    let tag_id = match db.tag_find(&args.tag)? {
        Some(id) => id,
        None => {
            return Err(AppError::BadInput {
                code: "tag_not_found".into(),
                message: format!("no tag named '{}'", args.tag),
                suggestion: "Run `mailing-list-cli tag ls` to see all tags".into(),
            });
        }
    };
    let removed = db.contact_tag_remove(contact_id, tag_id)?;
    output::success(
        format,
        &format!(
            "{} tag '{}' from {}",
            if removed { "removed" } else { "no-op;" },
            args.tag,
            args.email
        ),
        json!({
            "email": args.email,
            "tag": args.tag,
            "removed": removed
        }),
    );
    Ok(())
}

/// Look up a contact by email, return exit 3 if missing.
fn contact_id_or_fail(db: &Db, email: &str) -> Result<i64, AppError> {
    db.contact_find_id(email)?.ok_or_else(|| AppError::BadInput {
        code: "contact_not_found".into(),
        message: format!("no contact with email '{email}'"),
        suggestion: "Run `mailing-list-cli contact ls --list <id>` to find existing contacts"
            .into(),
    })
}
```

- [ ] **Step 3: Add the `contact_find_id` DB helper**

Add to `impl Db` in `src/db/mod.rs`:

```rust
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
```

- [ ] **Step 4: Add an integration test**

Append to `tests/cli.rs`:

```rust
#[test]
fn contact_tag_and_untag_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // seed: list + contact
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "contact", "add", "alice@example.com", "--list", "1",
        ]);
    cmd.assert().success();

    // tag
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "tag", "alice@example.com", "vip"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["tag"], "vip");

    // tag ls must show 1 member
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["tags"][0]["member_count"], 1);

    // untag
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "untag", "alice@example.com", "vip"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["removed"], true);
}

#[test]
fn contact_tag_on_missing_contact_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "tag", "ghost@example.com", "vip"]);
    cmd.assert().failure().code(3);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test contact_tag`
Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/commands/contact.rs src/db/mod.rs tests/cli.rs
git commit -m "feat(contact): contact tag / contact untag commands"
```

---

## Task 10: Field DB helpers + model

**Files:**
- Modify: `src/db/mod.rs`
- Modify: `src/models.rs`

- [ ] **Step 1: Add the `Field` model struct**

Append to `src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub id: i64,
    pub key: String,
    pub r#type: String,            // "text" | "number" | "date" | "bool" | "select"
    pub options: Option<Vec<String>>, // deserialized from options_json for select
    pub created_at: String,
}
```

- [ ] **Step 2: Add `field_*` DB helpers**

Add to `impl Db` in `src/db/mod.rs`:

```rust
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
                suggestion: "Use lowercase letters, digits, and underscores only (e.g. `company_size`)".into(),
            });
        }
        if !matches!(ty, "text" | "number" | "date" | "bool" | "select") {
            return Err(AppError::BadInput {
                code: "invalid_field_type".into(),
                message: format!("field type '{ty}' is not valid"),
                suggestion: "Use one of: text, number, date, bool, select".into(),
            });
        }
        if ty == "select"
            && options.map(|o| o.is_empty()).unwrap_or(true)
        {
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
            .prepare(
                "SELECT id, key, type, options_json, created_at FROM field ORDER BY key ASC",
            )
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
```

And add the helper function at the bottom of the file (outside any impl):

```rust
fn is_snake_case(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.starts_with('_')
        && !s.ends_with('_')
}
```

- [ ] **Step 3: Add unit tests for field helpers**

Append to the `tests` module in `src/db/mod.rs`:

```rust
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
        assert_eq!(fields[0].options.as_ref().unwrap(), &vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn field_delete_removes_and_cascades() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.field_create("company", "text", None).unwrap();
        assert!(db.field_delete("company").unwrap());
        assert!(db.field_get("company").unwrap().is_none());
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test db::tests::field`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/db/mod.rs src/models.rs
git commit -m "feat(db): field_* helpers with snake_case and select validation"
```

---

## Task 11: `field create`, `field ls`, `field rm` CLI

**Files:**
- Modify: `src/cli.rs` (replace `FieldAction` stub with full impl)
- Modify: `src/commands/field.rs`

- [ ] **Step 1: Expand `FieldAction` in `src/cli.rs`**

Replace the placeholder `FieldAction` enum with:

```rust
#[derive(Subcommand, Debug)]
pub enum FieldAction {
    /// Create a new custom field
    Create(FieldCreateArgs),
    /// List all custom fields
    #[command(visible_alias = "ls")]
    List,
    /// Delete a custom field (removes all stored values)
    Rm(FieldRmArgs),
}

#[derive(Args, Debug)]
pub struct FieldCreateArgs {
    /// Field key (snake_case, lowercase)
    pub key: String,
    /// Field type: text | number | date | bool | select
    #[arg(long, value_parser = ["text", "number", "date", "bool", "select"])]
    pub r#type: String,
    /// Comma-separated options for --type select
    #[arg(long)]
    pub options: Option<String>,
}

#[derive(Args, Debug)]
pub struct FieldRmArgs {
    /// Field key
    pub key: String,
    /// Explicit confirmation (required)
    #[arg(long)]
    pub confirm: bool,
}
```

- [ ] **Step 2: Implement the field command module**

Replace `src/commands/field.rs` with:

```rust
use crate::cli::{FieldAction, FieldCreateArgs, FieldRmArgs};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: FieldAction) -> Result<(), AppError> {
    let db = Db::open()?;
    match action {
        FieldAction::Create(args) => create(format, &db, args),
        FieldAction::List => list(format, &db),
        FieldAction::Rm(args) => remove(format, &db, args),
    }
}

fn create(format: Format, db: &Db, args: FieldCreateArgs) -> Result<(), AppError> {
    let options_vec: Option<Vec<String>> = args.options.as_ref().map(|s| {
        s.split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect()
    });
    let id = db.field_create(&args.key, &args.r#type, options_vec.as_deref())?;
    let field = db
        .field_get(&args.key)?
        .expect("field just created must exist");
    output::success(
        format,
        &format!("field created: {} ({})", args.key, args.r#type),
        json!({
            "id": id,
            "field": field
        }),
    );
    Ok(())
}

fn list(format: Format, db: &Db) -> Result<(), AppError> {
    let fields = db.field_all()?;
    let count = fields.len();
    output::success(
        format,
        &format!("{count} field(s)"),
        json!({ "fields": fields, "count": count }),
    );
    Ok(())
}

fn remove(format: Format, db: &Db, args: FieldRmArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("deleting field '{}' requires --confirm", args.key),
            suggestion: format!(
                "rerun with `mailing-list-cli field rm {} --confirm`",
                args.key
            ),
        });
    }
    let removed = db.field_delete(&args.key)?;
    if !removed {
        return Err(AppError::BadInput {
            code: "field_not_found".into(),
            message: format!("no field named '{}'", args.key),
            suggestion: "Run `mailing-list-cli field ls` to see existing fields".into(),
        });
    }
    output::success(
        format,
        &format!("field '{}' removed", args.key),
        json!({ "key": args.key, "removed": true }),
    );
    Ok(())
}
```

- [ ] **Step 3: Add integration tests**

Append to `tests/cli.rs`:

```rust
#[test]
fn field_create_list_rm_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // Create
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "company", "--type", "text"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["field"]["key"], "company");
    assert_eq!(v["data"]["field"]["type"], "text");

    // List
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);

    // Rm without --confirm fails
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "rm", "company"]);
    cmd.assert().failure().code(3);

    // Rm with --confirm succeeds
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "rm", "company", "--confirm"]);
    cmd.assert().success();
}

#[test]
fn field_create_select_without_options_fails() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "plan", "--type", "select"]);
    cmd.assert().failure().code(3);
}

#[test]
fn field_create_select_with_options_succeeds() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "field", "create", "plan", "--type", "select", "--options", "free,pro,enterprise",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["field"]["options"][1], "pro");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test field_`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/commands/field.rs tests/cli.rs
git commit -m "feat(field): field create/ls/rm with select options + snake_case validation"
```

---

## Task 12: Contact field value DB helpers (typed storage)

**Files:**
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Add typed upsert + read helpers**

Add to `impl Db` in `src/db/mod.rs`:

```rust
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
            "number" => raw.parse::<f64>().map(TypedFieldValue::Number).map_err(|_| {
                AppError::BadInput {
                    code: "field_coercion_failed".into(),
                    message: format!(
                        "field '{}' is type number but value '{}' is not numeric",
                        field.key, raw
                    ),
                    suggestion: "Provide a decimal number, e.g. 42 or 3.14".into(),
                }
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

    /// Fetch all field values for a contact, returned as (key, display_string).
    pub fn contact_fields_for(
        &self,
        contact_id: i64,
    ) -> Result<Vec<(String, String)>, AppError> {
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
```

Add the `TypedFieldValue` enum at the module level (near the bottom of `src/db/mod.rs`, outside `impl Db`):

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TypedFieldValue {
    Text(String),
    Number(f64),
    Date(String), // normalized RFC 3339 string
    Bool(bool),
}
```

- [ ] **Step 2: Add unit tests for coercion**

Append to the `tests` module in `src/db/mod.rs`:

```rust
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
            db.coerce_field_value(&f, "3.14").unwrap(),
            TypedFieldValue::Number(3.14)
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
```

- [ ] **Step 3: Run tests**

Run: `cargo test db::tests::coerce db::tests::contact_field_upsert`
Expected: 5 passed.

- [ ] **Step 4: Commit**

```bash
git add src/db/mod.rs
git commit -m "feat(db): typed contact_field_value upsert + coercion"
```

---

## Task 13: `contact set` CLI + `contact add --field key=val`

**Files:**
- Modify: `src/cli.rs` (add `ContactSetArgs`, extend `ContactAddArgs`)
- Modify: `src/commands/contact.rs`

- [ ] **Step 1: Extend CLI structs**

In `src/cli.rs`, replace `ContactAddArgs` with:

```rust
#[derive(Args, Debug)]
pub struct ContactAddArgs {
    /// Email address
    pub email: String,
    /// The list id to add the contact to
    #[arg(long)]
    pub list: i64,
    /// First name
    #[arg(long)]
    pub first_name: Option<String>,
    /// Last name
    #[arg(long)]
    pub last_name: Option<String>,
    /// Set a custom field value in `key=val` form; repeatable
    #[arg(long = "field", value_name = "KEY=VAL")]
    pub fields: Vec<String>,
}
```

Add `ContactSet` to `ContactAction`:

```rust
    /// Set a custom field value on a contact
    Set(ContactSetArgs),
```

Add at the bottom of the file:

```rust
#[derive(Args, Debug)]
pub struct ContactSetArgs {
    /// Contact email
    pub email: String,
    /// Field key
    pub field: String,
    /// Field value (coerced to the field's declared type)
    pub value: String,
}
```

- [ ] **Step 2: Add the `contact set` handler + extend `contact add`**

In `src/commands/contact.rs`, update `run`:

```rust
    match action {
        ContactAction::Add(args) => add(format, &db, &cli, args),
        ContactAction::List(args) => list_contacts(format, &db, args),
        ContactAction::Tag(args) => tag_contact(format, &db, args),
        ContactAction::Untag(args) => untag_contact(format, &db, args),
        ContactAction::Set(args) => set_field(format, &db, args),
    }
```

Add the `set_field` function:

```rust
fn set_field(
    format: Format,
    db: &Db,
    args: crate::cli::ContactSetArgs,
) -> Result<(), AppError> {
    let contact_id = contact_id_or_fail(db, &args.email)?;
    let field = db.field_get(&args.field)?.ok_or_else(|| AppError::BadInput {
        code: "field_not_found".into(),
        message: format!("no field named '{}'", args.field),
        suggestion: "Run `mailing-list-cli field ls`; create the field with `field create` first"
            .into(),
    })?;
    let typed = db.coerce_field_value(&field, &args.value)?;
    db.contact_field_upsert(contact_id, field.id, &typed)?;
    output::success(
        format,
        &format!("{}.{} = {}", args.email, args.field, args.value),
        json!({
            "email": args.email,
            "field": args.field,
            "value": args.value
        }),
    );
    Ok(())
}
```

Update the existing `add` function to accept `--field key=val` pairs. Replace the `add` function body with:

```rust
fn add(format: Format, db: &Db, cli: &EmailCli, args: ContactAddArgs) -> Result<(), AppError> {
    if !is_valid_email(&args.email) {
        return Err(AppError::BadInput {
            code: "invalid_email".into(),
            message: format!("'{}' is not a valid email address", args.email),
            suggestion: "Provide an email in the form local@domain".into(),
        });
    }

    // Parse --field key=val pairs into a validated (field, typed_value) list
    // BEFORE doing any DB writes. Fail fast on any coercion error.
    let mut typed_fields: Vec<(crate::models::Field, crate::db::TypedFieldValue)> = Vec::new();
    for pair in &args.fields {
        let (k, v) = pair.split_once('=').ok_or_else(|| AppError::BadInput {
            code: "invalid_field_arg".into(),
            message: format!("--field '{pair}' is not in key=val form"),
            suggestion: "Use `--field company=Acme`".into(),
        })?;
        let field = db.field_get(k)?.ok_or_else(|| AppError::BadInput {
            code: "field_not_found".into(),
            message: format!("no field named '{k}'"),
            suggestion: format!("Create the field first with `mailing-list-cli field create {k} --type text`"),
        })?;
        let typed = db.coerce_field_value(&field, v)?;
        typed_fields.push((field, typed));
    }

    let list = db
        .list_get_by_id(args.list)?
        .ok_or_else(|| AppError::BadInput {
            code: "list_not_found".into(),
            message: format!("no list with id {}", args.list),
            suggestion: "Run `mailing-list-cli list ls` to see all lists".into(),
        })?;

    // 1. Insert/upsert into local contact table
    let contact_id = db.contact_upsert(
        &args.email,
        args.first_name.as_deref(),
        args.last_name.as_deref(),
    )?;

    // 2. Write custom field values
    for (field, typed) in &typed_fields {
        db.contact_field_upsert(contact_id, field.id, typed)?;
    }

    // 3. Mirror to the Resend contact store
    cli.contact_create(
        &args.email,
        args.first_name.as_deref(),
        args.last_name.as_deref(),
        &[list.resend_segment_id.as_str()],
        None,
    )?;

    // 4. Wire up the local list_membership row
    db.contact_add_to_list(contact_id, list.id)?;

    output::success(
        format,
        &format!("added {} to list '{}'", args.email, list.name),
        json!({
            "contact_id": contact_id,
            "email": args.email,
            "list_id": list.id,
            "list_name": list.name,
            "fields_set": typed_fields.len()
        }),
    );
    Ok(())
}
```

- [ ] **Step 3: Add integration tests**

Append to `tests/cli.rs`:

```rust
#[test]
fn contact_set_with_typed_field_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // list + contact + field
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "add", "alice@example.com", "--list", "1"]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "age", "--type", "number"]);
    cmd.assert().success();

    // set numeric value
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "set", "alice@example.com", "age", "42"]);
    cmd.assert().success();

    // rejecting non-numeric value
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "set", "alice@example.com", "age", "old"]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_add_with_field_flags() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "field", "create", "company", "--type", "text"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "add",
            "alice@example.com",
            "--list",
            "1",
            "--field",
            "company=Acme",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["fields_set"], 1);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test contact_set contact_add_with_field`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/commands/contact.rs tests/cli.rs
git commit -m "feat(contact): contact set + contact add --field key=val with typed coercion"
```

---

## Task 14: `contact show` — full contact view

**Files:**
- Modify: `src/cli.rs` (add `ContactShowArgs`)
- Modify: `src/commands/contact.rs`
- Modify: `src/db/mod.rs` (add `contact_lists_for` helper)
- Modify: `src/models.rs`

- [ ] **Step 1: Add `Show` to `ContactAction` and `ContactShowArgs`**

In `src/cli.rs`, add to `enum ContactAction`:

```rust
    /// Show a contact's full details
    Show(ContactShowArgs),
```

At the bottom of the file:

```rust
#[derive(Args, Debug)]
pub struct ContactShowArgs {
    /// Contact email
    pub email: String,
}
```

- [ ] **Step 2: Add `contact_lists_for` DB helper**

Add to `impl Db` in `src/db/mod.rs`:

```rust
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
```

- [ ] **Step 3: Implement `contact show`**

In `src/commands/contact.rs`, add to the `run` match:

```rust
        ContactAction::Show(args) => show_contact(format, &db, args),
```

And add:

```rust
fn show_contact(
    format: Format,
    db: &Db,
    args: crate::cli::ContactShowArgs,
) -> Result<(), AppError> {
    let contact = db
        .contact_get_by_email(&args.email)?
        .ok_or_else(|| AppError::BadInput {
            code: "contact_not_found".into(),
            message: format!("no contact with email '{}'", args.email),
            suggestion: "Run `mailing-list-cli contact ls --list <id>` to browse contacts".into(),
        })?;
    let tags = db.contact_tags_for(contact.id)?;
    let fields = db.contact_fields_for(contact.id)?;
    let lists = db.contact_lists_for(contact.id)?;
    let fields_json: serde_json::Map<String, serde_json::Value> = fields
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let lists_json: Vec<serde_json::Value> = lists
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    output::success(
        format,
        &format!("contact: {}", contact.email),
        json!({
            "contact": contact,
            "tags": tags,
            "fields": fields_json,
            "lists": lists_json
        }),
    );
    Ok(())
}
```

- [ ] **Step 4: Add an integration test**

Append to `tests/cli.rs`:

```rust
#[test]
fn contact_show_returns_full_details() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed
    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "field", "create", "company", "--type", "text"],
        vec![
            "--json", "contact", "add", "alice@example.com", "--list", "1", "--first-name",
            "Alice", "--field", "company=Acme",
        ],
        vec!["--json", "contact", "tag", "alice@example.com", "vip"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    // Show
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "show", "alice@example.com"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["contact"]["email"], "alice@example.com");
    assert_eq!(v["data"]["contact"]["first_name"], "Alice");
    assert_eq!(v["data"]["tags"][0], "vip");
    assert_eq!(v["data"]["fields"]["company"], "Acme");
    assert_eq!(v["data"]["lists"][0]["name"], "news");
}

#[test]
fn contact_show_on_missing_email_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "show", "ghost@example.com"]);
    cmd.assert().failure().code(3);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test contact_show`
Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/commands/contact.rs src/db/mod.rs tests/cli.rs
git commit -m "feat(contact): contact show (tags, fields, list memberships)"
```

---

## Task 15: Segment DB helpers

**Files:**
- Modify: `src/db/mod.rs`
- Modify: `src/models.rs`

- [ ] **Step 1: Add the `Segment` model**

Append to `src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub id: i64,
    pub name: String,
    pub filter_json: String,
    pub created_at: String,
    pub member_count: i64,
}
```

- [ ] **Step 2: Add segment DB helpers**

Add to `impl Db` in `src/db/mod.rs`:

```rust
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
            .prepare(
                "SELECT id, name, filter_json, created_at FROM segment ORDER BY name ASC",
            )
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
            .query_row(
                &sql,
                rusqlite::params_from_iter(params.iter()),
                |r| r.get(0),
            )
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
```

- [ ] **Step 3: Add unit tests**

Append to the `tests` module:

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test db::tests::segment`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/db/mod.rs src/models.rs
git commit -m "feat(db): segment_* helpers with paginated member query"
```

---

## Task 16: `segment create` / `segment ls` CLI

**Files:**
- Modify: `src/cli.rs` (replace `SegmentAction` stub)
- Modify: `src/commands/segment.rs`

- [ ] **Step 1: Expand `SegmentAction`**

Replace the placeholder in `src/cli.rs` with:

```rust
#[derive(Subcommand, Debug)]
pub enum SegmentAction {
    /// Save a dynamic segment (a filter expression)
    Create(SegmentCreateArgs),
    /// List all segments
    #[command(visible_alias = "ls")]
    List,
    /// Show a segment's filter + sample members
    Show(SegmentShowArgs),
    /// List the contacts currently matching the segment
    Members(SegmentMembersArgs),
    /// Delete a segment definition (does not touch contacts)
    Rm(SegmentRmArgs),
}

#[derive(Args, Debug)]
pub struct SegmentCreateArgs {
    /// Segment name (used to reference it later)
    pub name: String,
    /// Filter expression, see `mailing-list-cli` docs §6 for grammar
    #[arg(long)]
    pub filter: String,
}

#[derive(Args, Debug)]
pub struct SegmentShowArgs {
    pub name: String,
}

#[derive(Args, Debug)]
pub struct SegmentMembersArgs {
    pub name: String,
    #[arg(long, default_value = "100")]
    pub limit: usize,
    #[arg(long)]
    pub cursor: Option<i64>,
}

#[derive(Args, Debug)]
pub struct SegmentRmArgs {
    pub name: String,
    #[arg(long)]
    pub confirm: bool,
}
```

- [ ] **Step 2: Implement `segment create` and `segment ls`**

Replace `src/commands/segment.rs` with:

```rust
use crate::cli::{
    SegmentAction, SegmentCreateArgs, SegmentMembersArgs, SegmentRmArgs, SegmentShowArgs,
};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use crate::segment::{compiler, parser};
use serde_json::json;

pub fn run(format: Format, action: SegmentAction) -> Result<(), AppError> {
    let db = Db::open()?;
    match action {
        SegmentAction::Create(args) => create(format, &db, args),
        SegmentAction::List => list(format, &db),
        SegmentAction::Show(args) => show(format, &db, args),
        SegmentAction::Members(args) => members(format, &db, args),
        SegmentAction::Rm(args) => remove(format, &db, args),
    }
}

fn parse_and_store(expr: &str) -> Result<(crate::segment::SegmentExpr, String), AppError> {
    let parsed = parser::parse(expr).map_err(|e| AppError::BadInput {
        code: "invalid_filter_expression".into(),
        message: e.message.clone(),
        suggestion: e.suggestion.clone(),
    })?;
    let filter_json = serde_json::to_string(&parsed).map_err(|e| AppError::Transient {
        code: "segment_serialize_failed".into(),
        message: format!("could not serialize SegmentExpr: {e}"),
        suggestion: "Report as a bug".into(),
    })?;
    Ok((parsed, filter_json))
}

fn create(format: Format, db: &Db, args: SegmentCreateArgs) -> Result<(), AppError> {
    let (_expr, filter_json) = parse_and_store(&args.filter)?;
    let id = db.segment_create(&args.name, &filter_json)?;
    output::success(
        format,
        &format!("segment created: {}", args.name),
        json!({
            "id": id,
            "name": args.name,
            "filter": args.filter
        }),
    );
    Ok(())
}

fn list(format: Format, db: &Db) -> Result<(), AppError> {
    let segments = db.segment_all()?;
    // Compute member counts per segment via the compiler
    let mut enriched = Vec::with_capacity(segments.len());
    for s in segments {
        let expr: crate::segment::SegmentExpr =
            serde_json::from_str(&s.filter_json).map_err(|e| AppError::Transient {
                code: "segment_deserialize_failed".into(),
                message: format!("corrupted segment '{}': {e}", s.name),
                suggestion: "Recreate the segment with `segment rm` + `segment create`".into(),
            })?;
        let (frag, params) = compiler::to_sql_where(&expr);
        let count = db.segment_count_members(&frag, &params)?;
        enriched.push(json!({
            "id": s.id,
            "name": s.name,
            "created_at": s.created_at,
            "member_count": count
        }));
    }
    let count = enriched.len();
    output::success(
        format,
        &format!("{count} segment(s)"),
        json!({ "segments": enriched, "count": count }),
    );
    Ok(())
}

fn show(format: Format, db: &Db, args: SegmentShowArgs) -> Result<(), AppError> {
    let segment = db.segment_get_by_name(&args.name)?.ok_or_else(|| AppError::BadInput {
        code: "segment_not_found".into(),
        message: format!("no segment named '{}'", args.name),
        suggestion: "Run `mailing-list-cli segment ls`".into(),
    })?;
    let expr: crate::segment::SegmentExpr =
        serde_json::from_str(&segment.filter_json).map_err(|e| AppError::Transient {
            code: "segment_deserialize_failed".into(),
            message: format!("corrupted segment: {e}"),
            suggestion: "Recreate the segment".into(),
        })?;
    let (frag, params) = compiler::to_sql_where(&expr);
    let member_count = db.segment_count_members(&frag, &params)?;
    let sample = db.segment_members(&frag, &params, 10, None)?;
    output::success(
        format,
        &format!("segment: {}", segment.name),
        json!({
            "id": segment.id,
            "name": segment.name,
            "filter_json": segment.filter_json,
            "filter_ast": expr,
            "created_at": segment.created_at,
            "member_count": member_count,
            "sample": sample
        }),
    );
    Ok(())
}

fn members(format: Format, db: &Db, args: SegmentMembersArgs) -> Result<(), AppError> {
    let segment = db.segment_get_by_name(&args.name)?.ok_or_else(|| AppError::BadInput {
        code: "segment_not_found".into(),
        message: format!("no segment named '{}'", args.name),
        suggestion: "Run `mailing-list-cli segment ls`".into(),
    })?;
    let expr: crate::segment::SegmentExpr =
        serde_json::from_str(&segment.filter_json).map_err(|e| AppError::Transient {
            code: "segment_deserialize_failed".into(),
            message: format!("corrupted segment: {e}"),
            suggestion: "Recreate the segment".into(),
        })?;
    let (frag, params) = compiler::to_sql_where(&expr);
    let contacts = db.segment_members(&frag, &params, args.limit, args.cursor)?;
    let next_cursor = contacts.last().map(|c| c.id);
    let count = contacts.len();
    output::success(
        format,
        &format!("{count} contact(s) in segment '{}'", segment.name),
        json!({
            "segment": segment.name,
            "contacts": contacts,
            "count": count,
            "next_cursor": next_cursor
        }),
    );
    Ok(())
}

fn remove(format: Format, db: &Db, args: SegmentRmArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("deleting segment '{}' requires --confirm", args.name),
            suggestion: format!(
                "rerun with `mailing-list-cli segment rm {} --confirm`",
                args.name
            ),
        });
    }
    if !db.segment_delete(&args.name)? {
        return Err(AppError::BadInput {
            code: "segment_not_found".into(),
            message: format!("no segment named '{}'", args.name),
            suggestion: "Run `mailing-list-cli segment ls`".into(),
        });
    }
    output::success(
        format,
        &format!("segment '{}' removed", args.name),
        json!({ "name": args.name, "removed": true }),
    );
    Ok(())
}
```

- [ ] **Step 3: Make `SegmentExpr` re-exported publicly**

The `segment` command module uses `crate::segment::SegmentExpr`. Confirm `src/segment/mod.rs` already re-exports it (it does, from Task 2). No change needed unless this step fails to compile.

- [ ] **Step 4: Add integration tests for segment create/ls/show/members/rm**

Append to `tests/cli.rs`:

```rust
#[test]
fn segment_create_list_show_members_round_trip() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed: list, contact, tag
    for args in [
        vec!["--json", "list", "create", "news"],
        vec![
            "--json", "contact", "add", "alice@example.com", "--list", "1",
        ],
        vec!["--json", "contact", "tag", "alice@example.com", "vip"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    // Create segment
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "create", "vips", "--filter", "tag:vip"]);
    cmd.assert().success();

    // List
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["segments"][0]["member_count"], 1);

    // Show
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "show", "vips"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["member_count"], 1);
    assert_eq!(v["data"]["sample"][0]["email"], "alice@example.com");

    // Members
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "members", "vips"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["contacts"][0]["email"], "alice@example.com");

    // Rm without --confirm fails
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "rm", "vips"]);
    cmd.assert().failure().code(3);

    // Rm with --confirm succeeds
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "rm", "vips", "--confirm"]);
    cmd.assert().success();
}

#[test]
fn segment_create_with_invalid_filter_returns_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "segment", "create", "bad", "--filter", "((unclosed",
        ]);
    cmd.assert().failure().code(3);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test segment_create_list segment_create_with_invalid`
Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/commands/segment.rs tests/cli.rs
git commit -m "feat(segment): create/ls/show/members/rm CLI with --confirm gate"
```

---

## Task 17: `contact ls --filter` + cursor pagination

**Files:**
- Modify: `src/cli.rs` (extend `ContactListArgs`)
- Modify: `src/commands/contact.rs`

- [ ] **Step 1: Extend `ContactListArgs`**

Replace the existing `ContactListArgs` in `src/cli.rs`:

```rust
#[derive(Args, Debug)]
pub struct ContactListArgs {
    /// Restrict to a list id (omit to search across all lists)
    #[arg(long)]
    pub list: Option<i64>,
    /// Filter expression (see the filter grammar reference)
    #[arg(long)]
    pub filter: Option<String>,
    /// Maximum number of contacts to return (max 10000)
    #[arg(long, default_value = "100")]
    pub limit: usize,
    /// Cursor (last contact id seen); start from the beginning if omitted
    #[arg(long)]
    pub cursor: Option<i64>,
}
```

- [ ] **Step 2: Rewrite `list_contacts` to use the compiler**

Replace the `list_contacts` function in `src/commands/contact.rs`:

```rust
fn list_contacts(format: Format, db: &Db, args: ContactListArgs) -> Result<(), AppError> {
    let limit = args.limit.min(10_000);

    // Build the base SQL fragment from filter + optional list restriction.
    // If both are present, AND them; if neither, match all contacts.
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(expr_text) = &args.filter {
        let parsed = crate::segment::parser::parse(expr_text).map_err(|e| AppError::BadInput {
            code: "invalid_filter_expression".into(),
            message: e.message.clone(),
            suggestion: e.suggestion.clone(),
        })?;
        let (frag, filter_params) = crate::segment::compiler::to_sql_where(&parsed);
        clauses.push(format!("({frag})"));
        params.extend(filter_params);
    }

    if let Some(list_id) = args.list {
        // Verify the list exists first so agents get a clear error instead of "0 results".
        let list = db
            .list_get_by_id(list_id)?
            .ok_or_else(|| AppError::BadInput {
                code: "list_not_found".into(),
                message: format!("no list with id {list_id}"),
                suggestion: "Run `mailing-list-cli list ls` to see all lists".into(),
            })?;
        clauses.push(
            "c.id IN (SELECT lm.contact_id FROM list_membership lm WHERE lm.list_id = ?)"
                .to_string(),
        );
        params.push(rusqlite::types::Value::Integer(list.id));
    }

    let fragment = if clauses.is_empty() {
        "1 = 1".to_string()
    } else {
        clauses.join(" AND ")
    };

    let contacts = db.segment_members(&fragment, &params, limit, args.cursor)?;
    let count = contacts.len();
    let next_cursor = contacts.last().map(|c| c.id);
    output::success(
        format,
        &format!("{count} contact(s)"),
        json!({
            "contacts": contacts,
            "count": count,
            "next_cursor": next_cursor,
            "limit": limit
        }),
    );
    Ok(())
}
```

Note: `ContactListArgs::list` is now `Option<i64>`; the earlier tests (Phase 2) used `--list 1` which still works. Verify by running existing integration tests.

- [ ] **Step 3: Add tests for `--filter` and pagination**

Append to `tests/cli.rs`:

```rust
#[test]
fn contact_ls_with_filter_returns_matching_subset() {
    let (_tmp, config_path, db_path) = stub_env();

    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "contact", "add", "alice@example.com", "--list", "1"],
        vec!["--json", "contact", "add", "bob@example.com", "--list", "1"],
        vec!["--json", "contact", "tag", "alice@example.com", "vip"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--filter", "tag:vip"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["contacts"][0]["email"], "alice@example.com");
}

#[test]
fn contact_ls_with_cursor_paginates() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed: 3 contacts
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();
    for email in ["a@ex.com", "b@ex.com", "c@ex.com"] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(["--json", "contact", "add", email, "--list", "1"]);
        cmd.assert().success();
    }

    // First page: limit 2
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--limit", "2"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 2);
    let cursor = v["data"]["next_cursor"].as_i64().unwrap();

    // Second page
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "contact", "ls", "--limit", "2", "--cursor", &cursor.to_string(),
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
    assert_eq!(v["data"]["contacts"][0]["email"], "c@ex.com");
}
```

- [ ] **Step 4: Verify existing `contact_add_then_contact_ls_round_trip` still works**

Run: `cargo test contact_add_then_contact_ls_round_trip contact_ls_with_filter contact_ls_with_cursor`
Expected: 3 passed. The older test passed `--list 1` which still works because `list` is now `Option<i64>` but clap parses the flag the same way.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/commands/contact.rs tests/cli.rs
git commit -m "feat(contact): contact ls --filter --list --limit --cursor pagination"
```

---

## Task 18: Parity integration test — `segment members` == `contact ls --filter`

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add a parity integration test**

Append to `tests/cli.rs`:

```rust
#[test]
fn segment_members_matches_contact_ls_filter() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed a non-trivial dataset: 4 contacts, 2 tags, varied memberships
    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "contact", "add", "alice@ex.com", "--list", "1", "--first-name", "Alice"],
        vec!["--json", "contact", "add", "bob@ex.com", "--list", "1"],
        vec!["--json", "contact", "add", "carol@ex.com", "--list", "1"],
        vec!["--json", "contact", "add", "dan@ex.com", "--list", "1"],
        vec!["--json", "contact", "tag", "alice@ex.com", "vip"],
        vec!["--json", "contact", "tag", "carol@ex.com", "vip"],
        vec!["--json", "contact", "tag", "alice@ex.com", "early"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    let filter = "tag:vip AND NOT tag:early";

    // Path 1: contact ls --filter
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--filter", filter]);
    let out = cmd.assert().success();
    let v_ls: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();

    // Path 2: segment create + segment members
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "create", "loyal", "--filter", filter]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "segment", "members", "loyal"]);
    let out = cmd.assert().success();
    let v_seg: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();

    // Emails should match exactly
    let ls_emails: Vec<String> = v_ls["data"]["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["email"].as_str().unwrap().to_string())
        .collect();
    let seg_emails: Vec<String> = v_seg["data"]["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["email"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ls_emails, seg_emails);
    assert_eq!(ls_emails, vec!["carol@ex.com".to_string()]);
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test segment_members_matches_contact_ls_filter`
Expected: 1 passed.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test: prove segment members and contact ls --filter are equivalent"
```

---

## Task 19: Fix `email_cli::contact_create` duplicate handling

**Files:**
- Modify: `src/email_cli.rs`
- Modify: `tests/fixtures/stub-email-cli.sh`

The current (v0.0.3) implementation silently swallows "already exists" errors from `email-cli contact create`. That means a contact added to list A and then to list B loses its segment membership in Resend: local DB tracks both memberships, but the Resend segment only has list A. When broadcasts ship in Phase 5 and target list B's Resend segment, list A-only contacts will silently drop off.

Fix: on duplicate, look up the existing Resend contact id and call `segment contact-add` for each requested segment.

- [ ] **Step 1: Add `segment_contact_add` wrapper to `EmailCli`**

Add to `impl EmailCli` in `src/email_cli.rs`:

```rust
    /// Add an existing Resend contact to a segment. Used by `contact_create`'s
    /// duplicate-handling path and by the CSV importer's re-run logic.
    pub fn segment_contact_add(&self, contact_email: &str, segment_id: &str) -> Result<(), AppError> {
        let output = Command::new(&self.path)
            .args([
                "--json",
                "segment",
                "contact-add",
                "--contact",
                contact_email,
                "--segment",
                segment_id,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli: {e}"),
                suggestion: "Check that email-cli is on PATH (v0.6+ required)".into(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "already in segment" is a successful no-op
            if stderr.contains("already") {
                return Ok(());
            }
            return Err(AppError::Transient {
                code: "segment_contact_add_failed".into(),
                message: format!(
                    "email-cli segment contact-add failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli segment list` to verify the segment exists".into(),
            });
        }
        Ok(())
    }
```

- [ ] **Step 2: Fix `contact_create` to ensure segment membership on duplicate**

Replace the tail of `contact_create` (the `if !output.status.success()` block and onward) with:

```rust
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let is_duplicate = stderr.contains("already exists") || stderr.contains("duplicate");
            if is_duplicate {
                // The contact already exists in Resend. Our local DB is the
                // source of truth for memberships, so ensure the existing
                // Resend contact is in each requested segment.
                for seg in segments {
                    self.segment_contact_add(email, seg)?;
                }
                return Ok(());
            }
            return Err(AppError::Transient {
                code: "contact_create_failed".into(),
                message: format!(
                    "email-cli contact create failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli contact list` to inspect Resend contact state".into(),
            });
        }

        Ok(())
    }
```

- [ ] **Step 3: Add a unit test that exercises the duplicate path**

The existing unit test (`missing_email_cli_returns_config_error`) doesn't cover this path because it needs a real subprocess. Add an integration test via the stub instead (next step).

- [ ] **Step 4: Extend `stub-email-cli.sh` for the duplicate path**

Replace the `contact` block in `tests/fixtures/stub-email-cli.sh` with:

```sh
    "contact")
        case "$2" in
            "create")
                # If MLC_STUB_CONTACT_DUPLICATE is set, simulate a duplicate
                if [ -n "$MLC_STUB_CONTACT_DUPLICATE" ]; then
                    echo "contact already exists" >&2
                    exit 1
                fi
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890"}}'
                exit 0
                ;;
            "list")
                echo '{"version":"1","status":"success","data":{"object":"list","data":[]}}'
                exit 0
                ;;
            "get"|"show")
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890","email":"stub@example.com"}}'
                exit 0
                ;;
            "update")
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890"}}'
                exit 0
                ;;
            "delete"|"rm")
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890","deleted":true}}'
                exit 0
                ;;
        esac
        ;;
```

Also, ensure the `segment` block includes `contact-add`:

```sh
    "segment")
        case "$2" in
            "create")
                echo '{"version":"1","status":"success","data":{"id":"seg_test_12345","name":"stub"}}'
                exit 0
                ;;
            "list")
                echo '{"version":"1","status":"success","data":{"object":"list","data":[]}}'
                exit 0
                ;;
            "contact-add"|"contact-remove")
                echo '{"version":"1","status":"success","data":{"id":"seg_test_12345"}}'
                exit 0
                ;;
        esac
        ;;
```

- [ ] **Step 5: Add an integration test for the duplicate-contact path**

Append to `tests/cli.rs`:

```rust
#[test]
fn contact_add_duplicate_triggers_segment_contact_add() {
    let (_tmp, config_path, db_path) = stub_env();

    // Create list
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    // Simulate duplicate on the Resend side; the local DB should still succeed
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_STUB_CONTACT_DUPLICATE", "1")
        .args(["--json", "contact", "add", "alice@example.com", "--list", "1"]);
    cmd.assert().success();

    // Verify local membership
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--list", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test contact_add_duplicate_triggers`
Expected: 1 passed.

Also run the full contact suite to confirm nothing regressed:

Run: `cargo test --test cli`
Expected: all tests green.

- [ ] **Step 7: Commit**

```bash
git add src/email_cli.rs tests/fixtures/stub-email-cli.sh tests/cli.rs
git commit -m "fix(email_cli): on duplicate contact, ensure segment membership via segment contact-add"
```

---

## Task 20: CSV importer library

**Files:**
- Create: `src/csv_import.rs`
- Modify: `src/main.rs` (add `mod csv_import;`)

The importer is pure Rust — no CLI surface yet. Task 21 wires it into `contact import`.

- [ ] **Step 1: Create the module**

Create `src/csv_import.rs`:

```rust
//! Streaming CSV import for `contact import`.
//!
//! Spec §9.3: every row must carry a `consent_source` column, unless the caller
//! passes `--unsafe-no-consent` (which tags every row `imported_without_consent`).
//! Resumability contract: the importer is idempotent under replay — running the
//! same file twice produces no duplicate contacts, tags, or field values.

use crate::db::{Db, TypedFieldValue};
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
pub fn read_rows<R: Read>(
    reader: R,
    unsafe_no_consent: bool,
) -> Result<Vec<ImportRow>, AppError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(reader);

    let headers = rdr
        .headers()
        .map_err(|e| AppError::BadInput {
            code: "csv_header_error".into(),
            message: format!("could not read CSV headers: {e}"),
            suggestion: "Verify the file starts with a header row like `email,first_name,...`".into(),
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
    let well_known: Vec<usize> =
        [Some(email_idx), consent_idx, first_name_idx, last_name_idx, tags_idx]
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
        let email = rec
            .get(email_idx)
            .map(str::to_string)
            .unwrap_or_default();
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
        assert_eq!(rows[0].fields, vec![("company".to_string(), "Acme".to_string())]);
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
        let contact = db.contact_get_by_email("alice@example.com").unwrap().unwrap();
        assert!(
            db.contact_tags_for(contact.id)
                .unwrap()
                .contains(&"imported_without_consent".to_string())
        );
    }
}
```

- [ ] **Step 2: Wire into `main.rs`**

Add `mod csv_import;` to `src/main.rs` alongside the existing module declarations.

- [ ] **Step 3: Run the importer tests**

Run: `cargo test csv_import`
Expected: 8 passed.

- [ ] **Step 4: Commit**

```bash
git add src/csv_import.rs src/main.rs
git commit -m "feat(csv_import): streaming reader + idempotent row applier with consent enforcement"
```

---

## Task 21: `contact import` CLI + rate limiting

**Files:**
- Modify: `src/cli.rs` (add `ContactImportArgs`)
- Modify: `src/commands/contact.rs`
- Modify: `src/email_cli.rs` (add throttle)

- [ ] **Step 1: Add subprocess throttle to `EmailCli`**

Add a `std::sync::Mutex<std::time::Instant>` field to `EmailCli` and a `throttle()` helper. Replace the `struct EmailCli` definition at the top of `src/email_cli.rs`:

```rust
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

/// A handle to the local email-cli binary.
pub struct EmailCli {
    pub path: String,
    pub profile: String,
    last_call: Mutex<Instant>,
}

const MIN_INTERVAL: StdDuration = StdDuration::from_millis(200);

impl EmailCli {
    pub fn new(path: impl Into<String>, profile: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            profile: profile.into(),
            last_call: Mutex::new(Instant::now() - MIN_INTERVAL),
        }
    }

    /// Sleep until at least 200ms have elapsed since the last call. This
    /// enforces the 5 req/sec Resend rate limit at the subprocess layer
    /// across ALL invocations.
    fn throttle(&self) {
        let mut last = self.last_call.lock().unwrap();
        let elapsed = last.elapsed();
        if elapsed < MIN_INTERVAL {
            std::thread::sleep(MIN_INTERVAL - elapsed);
        }
        *last = Instant::now();
    }
```

Then add `self.throttle();` as the FIRST line inside `agent_info`, `segment_create`, `contact_create`, `segment_contact_add`, and `profile_test`. Example for `contact_create`:

```rust
    pub fn contact_create(
        &self,
        email: &str,
        first_name: Option<&str>,
        last_name: Option<&str>,
        segments: &[&str],
        properties: Option<&Value>,
    ) -> Result<(), AppError> {
        self.throttle();
        let mut args: Vec<String> = vec![
            // ... unchanged ...
```

- [ ] **Step 2: Extend `ContactAction` with `Import`**

In `src/cli.rs`, add to `enum ContactAction`:

```rust
    /// Bulk-import contacts from a CSV file
    Import(ContactImportArgs),
```

At the bottom:

```rust
#[derive(Args, Debug)]
pub struct ContactImportArgs {
    /// Path to the CSV file
    pub file: std::path::PathBuf,
    /// The list id to add every imported row to
    #[arg(long)]
    pub list: i64,
    /// Send a double opt-in confirmation (Phase 7 feature; errors in Phase 3)
    #[arg(long = "double-opt-in")]
    pub double_opt_in: bool,
    /// Allow import without per-row consent (adds `imported_without_consent` tag)
    #[arg(long = "unsafe-no-consent")]
    pub unsafe_no_consent: bool,
}
```

- [ ] **Step 3: Implement the `contact import` command**

In `src/commands/contact.rs`, add to the `run` match:

```rust
        ContactAction::Import(args) => import(format, &db, &cli, args),
```

Add the `import` function:

```rust
fn import(
    format: Format,
    db: &Db,
    cli: &EmailCli,
    args: crate::cli::ContactImportArgs,
) -> Result<(), AppError> {
    // Defer real DOI to Phase 7.
    if args.double_opt_in {
        return Err(AppError::BadInput {
            code: "double_opt_in_not_available".into(),
            message: "--double-opt-in requires `optin start`/`verify` which ship in v0.1.3 (Phase 7)".into(),
            suggestion: "Rerun without --double-opt-in; for now imported contacts default to status=active".into(),
        });
    }

    let list = db.list_get_by_id(args.list)?.ok_or_else(|| AppError::BadInput {
        code: "list_not_found".into(),
        message: format!("no list with id {}", args.list),
        suggestion: "Run `mailing-list-cli list ls`".into(),
    })?;

    // Read + validate all rows before touching the DB so a malformed file
    // never leaves a half-imported state.
    let file = std::fs::File::open(&args.file).map_err(|e| AppError::BadInput {
        code: "csv_open_failed".into(),
        message: format!("could not open {}: {e}", args.file.display()),
        suggestion: "Check the file path and permissions".into(),
    })?;
    let rows = crate::csv_import::read_rows(file, args.unsafe_no_consent)?;

    let total = rows.len();
    let mut summary = crate::csv_import::ImportSummary {
        total_rows: total,
        ..Default::default()
    };

    for (idx, row) in rows.iter().enumerate() {
        // Local write first
        match crate::csv_import::apply_row_local(db, list.id, row, args.unsafe_no_consent) {
            Ok(()) => {
                summary.inserted += 1;
                if args.unsafe_no_consent {
                    summary.tagged_without_consent += 1;
                }
            }
            Err(e) if e.code() == "contact_suppressed" => {
                summary.skipped_suppressed += 1;
                continue;
            }
            Err(e) => {
                summary.skipped_invalid += 1;
                summary
                    .errors
                    .push(format!("row {}: {}", idx + 2, e.message()));
                continue;
            }
        }

        // Mirror to Resend (rate-limited at the subprocess layer).
        // Failures don't abort the import — they roll up into the summary.
        if let Err(e) = cli.contact_create(
            &row.email,
            row.first_name.as_deref(),
            row.last_name.as_deref(),
            &[list.resend_segment_id.as_str()],
            None,
        ) {
            summary
                .errors
                .push(format!("row {}: {} (resend mirror)", idx + 2, e.message()));
        }

        // Progress to stderr every 100 rows
        if (idx + 1) % 100 == 0 {
            eprintln!("imported {}/{}", idx + 1, total);
        }
    }

    output::success(
        format,
        &format!(
            "imported {} of {} rows ({} suppressed, {} errors)",
            summary.inserted, summary.total_rows, summary.skipped_suppressed, summary.skipped_invalid
        ),
        json!({
            "total_rows": summary.total_rows,
            "inserted": summary.inserted,
            "skipped_suppressed": summary.skipped_suppressed,
            "skipped_invalid": summary.skipped_invalid,
            "tagged_without_consent": summary.tagged_without_consent,
            "errors": summary.errors
        }),
    );
    Ok(())
}
```

- [ ] **Step 4: Add integration tests for the importer**

Append to `tests/cli.rs`:

```rust
#[test]
fn contact_import_happy_path() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("contacts.csv");
    std::fs::write(
        &csv_path,
        "email,first_name,consent_source\n\
         alice@example.com,Alice,landing\n\
         bob@example.com,Bob,manual\n",
    )
    .unwrap();

    // Create list first
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    // Import
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["inserted"], 2);
    assert_eq!(v["data"]["skipped_suppressed"], 0);
}

#[test]
fn contact_import_rejects_missing_consent_source() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("nocnt.csv");
    std::fs::write(&csv_path, "email,first_name\nalice@example.com,Alice\n").unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "contact", "import", csv_path.to_str().unwrap(), "--list", "1",
        ]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_import_unsafe_no_consent_tags_rows() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("nocnt.csv");
    std::fs::write(&csv_path, "email\nalice@example.com\n").unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "contact", "import", csv_path.to_str().unwrap(), "--list", "1",
            "--unsafe-no-consent",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["tagged_without_consent"], 1);

    // And the tag actually landed
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "tag", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let tags: Vec<String> = v["data"]["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(tags.contains(&"imported_without_consent".to_string()));
}

#[test]
fn contact_import_rejects_double_opt_in_flag_in_phase_3() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("doi.csv");
    std::fs::write(&csv_path, "email,consent_source\nalice@example.com,manual\n").unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "contact",
            "import",
            csv_path.to_str().unwrap(),
            "--list",
            "1",
            "--double-opt-in",
        ]);
    cmd.assert().failure().code(3);
}

#[test]
fn contact_import_rerun_is_idempotent() {
    let (tmp, config_path, db_path) = stub_env();
    let csv_path = tmp.path().join("contacts.csv");
    std::fs::write(
        &csv_path,
        "email,consent_source\nalice@example.com,manual\nbob@example.com,manual\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "list", "create", "news"]);
    cmd.assert().success();

    for _ in 0..3 {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args([
                "--json", "contact", "import", csv_path.to_str().unwrap(), "--list", "1",
            ]);
        cmd.assert().success();
    }

    // Still exactly 2 contacts
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "contact", "ls", "--list", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 2);
}
```

- [ ] **Step 5: Run the importer tests**

Run: `cargo test contact_import`
Expected: 5 passed.

Note: each test has ~2-3 `contact_create` calls that incur a 200ms throttle sleep, so these tests take a few seconds. That's fine.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/commands/contact.rs src/email_cli.rs tests/cli.rs
git commit -m "feat(contact): contact import with rate limit, consent enforcement, idempotent replay"
```

---

## Task 22: Update agent-info manifest for v0.0.4

**Files:**
- Modify: `src/commands/agent_info.rs`

- [ ] **Step 1: Rewrite the manifest with every Phase 3 command**

Replace the `commands` block in `src/commands/agent_info.rs`:

```rust
        "commands": {
            "agent-info": "Machine-readable capability manifest (this output)",
            "health": "Run a system health check",
            "list create <name> [--description <text>]": "Create a list (backed by a Resend segment via email-cli)",
            "list ls": "List all lists with subscriber counts",
            "list show <id>": "Show one list's details",
            "contact add <email> --list <id> [--first-name F --last-name L --field key=val ...]": "Add a contact to a list",
            "contact ls [--list <id>] [--filter <expr>] [--limit N] [--cursor C]": "List/filter contacts",
            "contact show <email>": "Show a contact's full details (tags, fields, list memberships)",
            "contact tag <email> <tag>": "Apply a tag to a contact",
            "contact untag <email> <tag>": "Remove a tag from a contact",
            "contact set <email> <field> <value>": "Set a typed custom field value",
            "contact import <file.csv> --list <id> [--unsafe-no-consent]": "Bulk-import contacts from CSV (5 req/sec rate limit, idempotent replay)",
            "tag ls": "List all tags with member counts",
            "tag rm <name> --confirm": "Delete a tag",
            "field create <key> --type <text|number|date|bool|select> [--options a,b,c]": "Create a typed custom field",
            "field ls": "List all custom fields",
            "field rm <key> --confirm": "Delete a custom field",
            "segment create <name> --filter <expr>": "Save a dynamic segment",
            "segment ls": "List all segments with member counts",
            "segment show <name>": "Show a segment's filter + 10 sample members",
            "segment members <name> [--limit N] [--cursor C]": "List contacts currently matching the segment",
            "segment rm <name> --confirm": "Delete a segment definition",
            "update [--check]": "Self-update from GitHub Releases",
            "skill install": "Install skill files into Claude / Codex / Gemini paths",
            "skill status": "Show which platforms have the skill installed"
        },
```

Update the status line:

```rust
        "status": "v0.0.4 — contacts, tags, fields, segments, filter parser, CSV import"
```

- [ ] **Step 2: Update the agent-info integration test**

Update the existing `agent_info_lists_health_command` test in `tests/cli.rs` (or add a new test) to verify the new commands are listed:

```rust
#[test]
fn agent_info_lists_phase_3_commands() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&stdout).unwrap();
    let commands = v["commands"].as_object().unwrap();
    // Sanity: every major Phase 3 command is advertised
    for key in [
        "contact show <email>",
        "contact tag <email> <tag>",
        "contact import <file.csv> --list <id> [--unsafe-no-consent]",
        "tag ls",
        "field create <key> --type <text|number|date|bool|select> [--options a,b,c]",
        "segment create <name> --filter <expr>",
        "segment members <name> [--limit N] [--cursor C]",
    ] {
        assert!(
            commands.contains_key(key),
            "agent-info missing command: {key}"
        );
    }
    assert!(v["status"].as_str().unwrap().starts_with("v0.0.4"));
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test agent_info`
Expected: 3 passed (existing + new).

- [ ] **Step 4: Commit**

```bash
git add src/commands/agent_info.rs tests/cli.rs
git commit -m "feat(agent-info): advertise phase 3 commands; v0.0.4 status"
```

---

## Task 23: Parity plan amendment + version bump + final sweep + tag

**Files:**
- Modify: `Cargo.toml` (version bump)
- Modify: `docs/plans/2026-04-08-parity-plan.md` (remove erase/resubscribe from Phase 3)
- Modify: `README.md` (update the badge if needed)

- [ ] **Step 1: Bump the crate version**

Edit `Cargo.toml`:

```toml
version = "0.0.4"
```

- [ ] **Step 2: Amend the parity plan**

Edit `docs/plans/2026-04-08-parity-plan.md`. In §5 Phase 3 "Ships:", replace:

```
- `contact show <email>`, `contact erase <email>`, `contact resubscribe <email>`
```

with:

```
- `contact show <email>` (contact erase and contact resubscribe deferred to Phase 7 — they depend on suppression CRUD and the audit log that Phase 7 owns)
```

And replace:

```
- `contact import <file.csv> --list <id> [--double-opt-in]` with rate-limit-aware chunking
```

with:

```
- `contact import <file.csv> --list <id> [--unsafe-no-consent]` with rate-limit-aware chunking at the subprocess layer and idempotent replay (--double-opt-in is visible but rejected with exit 3 until Phase 7)
```

- [ ] **Step 3: Update the README release badge (if present)**

Check `README.md` for a shields.io badge like `v0.0.3_email--cli_v0.6` and update to `v0.0.4`. If none, skip this step.

Run: `grep -n "v0\.0\.[0-9]" README.md`

If a match is found, edit it via the `Edit` tool.

- [ ] **Step 4: Full test sweep**

Run: `cargo fmt --check`
Expected: clean

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean

Run: `cargo test`
Expected: all green. At least ~55 unit tests + ~25 integration tests.

- [ ] **Step 5: Commit the version bump + parity plan amendment**

```bash
git add Cargo.toml Cargo.lock docs/plans/2026-04-08-parity-plan.md README.md
git commit -m "chore: bump to v0.0.4 + parity plan amendment (defer erase/resubscribe)"
```

- [ ] **Step 6: Push + tag**

```bash
git push origin main
git tag -a v0.0.4 -m "v0.0.4 — contacts, tags, fields, segments, filter parser, CSV import"
git push origin v0.0.4
```

- [ ] **Step 7: Verify CI went green**

```bash
gh run list --repo paperfoot/mailing-list-cli --limit 1
```

If the latest run status is `in_progress`, wait and re-check. If `completed, success`, you're done.

---

## What Phase 3 does NOT ship (scope discipline)

These would all be plausible additions but are explicitly deferred. Attempting to include them in Phase 3 would break the "ships a v0.0.4 tag this week" goal.

1. **`contact erase`** — deferred to Phase 7 (needs suppression CRUD + audit log + `email-cli contact delete`).
2. **`contact resubscribe`** — deferred to Phase 7 (needs suppression CRUD).
3. **Real `--double-opt-in`** — deferred to Phase 7 (needs `optin start`/`verify`/`pending` and token table wiring). Phase 3 errors with exit 3 if the flag is used.
4. **`suppression` CRUD** (`suppression ls/add/rm/import/export`) — deferred to Phase 7.
5. **`unsubscribe` command** — deferred to Phase 7.
6. **Resend property schema sync** (`contact-property create`) — local fields only in Phase 3.
7. **Engagement cache** (`contact.last_active_at`) — deferred to Phase 6 when events actually start flowing from the webhook listener.
8. **`governor`-style async rate limiting** — the synchronous mutex-guarded 200ms sleep is simpler and sufficient for Phase 3's single-process importer.
9. **Email syntax validation beyond the basic is_valid_email check** — deferred to Phase 7 `dnscheck`.
10. **`import_jobs` table with durable cursor** — Phase 3 uses idempotent replay instead.

If any of these come up during implementation, file a parity-plan amendment rather than sneaking them in.

---

## Acceptance criteria (must all pass before tagging v0.0.4)

Copy this list into the commit body of the v0.0.4 tag for auditability.

- [ ] Every task above is checked off.
- [ ] `cargo fmt --check` is clean.
- [ ] `cargo clippy --all-targets -- -D warnings` is clean.
- [ ] `cargo test` passes ≥ 55 unit tests + ≥ 25 integration tests. (Earlier counts: Phase 1 = 13 unit / 7 integ; Phase 2 = 18 unit / 12 integ; Phase 3 adds roughly 40 unit + 20 integration.)
- [ ] The filter parser handles every example expression in spec §6.2.
- [ ] `segment members <name>` and `contact ls --filter <expr>` return identical result sets on the parity test dataset (Task 18).
- [ ] `contact import` is idempotent: running the same CSV 3 times produces exactly one set of contacts/tags/fields.
- [ ] `contact import` refuses any CSV missing `consent_source` unless `--unsafe-no-consent` is passed.
- [ ] `contact import --double-opt-in` returns exit 3 with a message pointing to Phase 7.
- [ ] `email_cli::contact_create` correctly calls `segment contact-add` on the "already exists" path (Task 19 test).
- [ ] `agent-info` advertises every Phase 3 command and reports status `v0.0.4 …`.
- [ ] The Cargo.toml version is `0.0.4` and `v0.0.4` tag is pushed.
- [ ] The parity plan's §5 Phase 3 section has been amended to reflect the deferred scope.

---

## Self-review summary (applied before execution)

This plan was reviewed in parallel by Codex (`gpt-5.4` with `model_reasoning_effort=xhigh`) and Gemini 3.1 Pro before the final draft was committed. The key corrections folded in:

- **Deferred `contact erase` + `contact resubscribe` to Phase 7** (both reviewers agreed a stubbed version creates fake compliance surface).
- **`--double-opt-in` is visible but rejected** rather than stubbed (removes accidental false-positive DOI semantics).
- **`contact set` is local-only** (no sync-to-resend plumbing without a schema migration).
- **`segment rm`, `--options` on `field create`, `--cursor` on `contact ls`, `--field key=val` on `contact add`** — all added to match spec §4.
- **`email_cli::contact_create` duplicate path fix** (Task 19) — the previously silent "already exists" swallow was a latent Phase 5 bug.
- **CSV consent enforcement** (spec §9.3) is now a hard invariant of Task 20.
- **Parity test** between `segment members` and `contact ls --filter` (Task 18) proves the two consumers share one code path.
- **Rate limit at subprocess layer** (`EmailCli::throttle`) not row layer, so any future command that calls email-cli inherits the 5 req/sec budget.

Deferred suggestions (Gemini):
- `governor`-crate async rate limiter (Phase 3 scope: sync mutex is simpler)
- `contact.last_active_at` cached column with triggers (Phase 6 scope: depends on events)
- `proptest` fuzzing (Phase 9 scope: polish)
- Email syntax verifier / MX lookup (Phase 7 scope: `dnscheck`)

---

*End of Phase 3 implementation plan.*

