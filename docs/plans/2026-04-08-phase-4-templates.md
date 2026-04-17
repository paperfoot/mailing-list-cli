# Phase 4: Templates + Agent Authoring Guide — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `mailing-list-cli v0.1.0` — a first-class template subsystem (MJML + Handlebars + CSS inlining + plain-text alt), a lint rule set that catches the most common cross-client bugs, and the embedded agent authoring guide surfaced via `template guidelines`. After this phase, an LLM can author an MJML template that renders correctly in Gmail, Apple Mail, and Outlook desktop on its first try.

**Architecture:** A new `src/template/` module owns the template pipeline. Templates are YAML-frontmatter-prefixed MJML with Mustache `{{ }}` merge tags, compiled in-process via `mrml` (pure Rust). The compile pipeline: parse frontmatter → render Handlebars → parse MJML → render to HTML → inline CSS (`css-inline`) → generate plain-text alt (`html2text`) → return `{html, text, subject}`. Templates persist in the existing `template` table. The authoring guide is compiled into the binary via `include_str!` from `assets/template-authoring.md`.

**Tech Stack:** Rust 2024 edition, MSRV 1.85. New deps: `mrml 5.1`, `handlebars 6.4`, `css-inline 0.20`, `html2text 0.16`, `serde_yaml 0.9`. Existing: `clap 4.5`, `rusqlite 0.37` (bundled), `serde`, `thiserror`, `chrono`.

**Note:** the earlier draft of this plan pulled in `gray_matter` for frontmatter parsing. Codex review [REMOVE] pushed back — it's one more dependency for a problem that needs ~20 lines of manual split + `serde_yaml::from_str`. We do it by hand. See Task 2.

**Spec reference:** [`docs/specs/2026-04-07-mailing-list-cli-design.md`](../specs/2026-04-07-mailing-list-cli-design.md) §4.5 (template commands), §7 (compile pipeline + lint rules), §16 (embedded authoring guide text). [`docs/plans/2026-04-08-parity-plan.md`](./2026-04-08-parity-plan.md) §5 Phase 4.

**Prerequisite:** Phase 3 is complete (`v0.0.4` tagged). The `template` schema table is ALREADY in place from migration `0001_initial`. No schema work in this phase.

---

## Locked design decisions (read before Task 1)

These reflect the critical fixes from parallel Codex + Gemini review of an earlier draft.

1. **MJML via `mrml`, not React Email / jsx-email / Maizzle.** Pure Rust, no Node, no JIT. Cold-starts in microseconds. Research dossier 05 settled this debate — we're not revisiting it.
2. **Merge tags use Mustache-style `{{ snake_case }}`** rendered by `handlebars 6.4`. Triple-brace `{{{ }}}` is reserved for an **allowlist of exactly two placeholders**: `unsubscribe_link` and `physical_address_footer`. Every other triple-brace occurrence is a lint ERROR (XSS risk via contact field injection).
3. **`handlebars` is configured with `set_strict_mode(true)`**. Undeclared/missing variables raise an error at render time. Silent drops are the #1 merge-tag footgun and we refuse to inherit them. Declared-but-missing variables in `template render --with-data` return exit 3 with the list of missing vars.
4. **Frontmatter is parsed by hand, not `gray_matter`.** `split_frontmatter` reads the first line, requires `---\n`, scans for the second `---\n` or `---\r\n` delimiter, passes the YAML block to `serde_yaml::from_str::<VarSchema>()`, and returns the body. Deterministic errors with line numbers, one fewer dependency. [Codex REMOVE #3]
5. **`{{{ unsubscribe_link }}}` and `{{{ physical_address_footer }}}` are NOT substituted in Phase 4's default render.** They are required string occurrences that the linter verifies are present. The actual substitution happens at send time (Phase 5) when the broadcast knows the recipient and the config knows the physical address. In Phase 4, `template render` passes them through as literal triple-brace text — and a special `--with-placeholders` flag substitutes stub values for preview.
6. **Compile pipeline is pure (no DB access).** `src/template/compile.rs::compile(source: &str, data: &Value) -> Result<Rendered, CompileError>` takes only the raw source + merge data. The CLI layer is responsible for loading from DB + marshalling config.
7. **Frozen template language subset.** Phase 4 supports ONLY: scalar `{{ var }}`, triple-brace allowlist `{{{ unsubscribe_link }}}` + `{{{ physical_address_footer }}}`, `{{#if var}} ... {{else}} ... {{/if}}`, `{{#unless var}} ... {{/unless}}`. **NOT supported:** `{{#each}}`, custom helpers, partials, `<mj-include>`. The lint flags any `{{#each}}` or `{{> partial}}` as an error. `<mj-include>` is a lint error because templates live in SQLite and there's no file path to resolve. [Codex FIX #11]
8. **Reserved variables are auto-injected at send time, never declared in frontmatter.** List: `first_name`, `last_name`, `email`, `current_year`, `broadcast_id`. They use normal `{{ name }}` braces (single escape). The linter treats them as built-in — using them is fine, declaring them is a no-op warning. The two triple-brace placeholders (`unsubscribe_link`, `physical_address_footer`) are a separate allowlist. [Codex QUESTION #12]
9. **`template lint` exit codes.**
   - `0` if zero errors (warnings allowed, included in JSON output)
   - `3` if any error-severity finding (including frontmatter invalid, MJML parse failure, undeclared var, forbidden tag, missing required placeholder)
   - Any exit code 4 is reserved for RateLimited (global AppError contract)
   Even catastrophic MJML parse failures exit 3 — consistent with the rest of the CLI surface. [Codex FIX #7, Gemini #7]
10. **`template edit` is strictly guarded.** It is the only interactive command in the crate. It requires:
    - stdout is a TTY (fail fast in `--json`, pipes, or CI with exit 3)
    - `$VISUAL` or `$EDITOR` environment variable set (no silent `vi` fallback)
    - Editor invoked directly via `Command::new(editor_path).arg(tempfile)` — NO shell interpolation
    - Temp file written to a fresh tempdir, then read back after the editor exits
    - Re-lint the result; reject save on errors unless `--force`
    - Atomic update of the DB row (via the existing upsert path)
    Documented in the command help that this is the only interactive command. [Codex FIX #9]
11. **HTML size lint thresholds** (post-inline measurement):
    - ≥ 90_000 bytes → **warning** (nearing Gmail clip)
    - ≥ 102_000 bytes → **error** (Gmail will clip the footer, hiding the unsubscribe link — that's a compliance failure, not an aesthetics issue) [Codex FIX #7]
12. **Template names are snake_case**, lowercase alphanumerics + underscore. Same rule as `field.key` from Phase 3. Enforced in `template_upsert` DB helper via the existing `is_snake_case` free function (reused, not duplicated).
13. **Preview text (`<mj-preview>`)** is a recommended-not-required component — linter warns if missing, does not error.
14. **`template render --with-data` always outputs JSON envelope** with `{ subject, html, text, size_bytes, lint_warnings, lint_errors }`. The HTML is embedded as a string in the JSON, not streamed. Agents parse it; humans can pipe to `| jq -r '.data.html'` for raw output. [Codex FIX #13]
15. **Authoring guide is bundled via `include_str!`** from `assets/template-authoring.md`. Total size ~5 KB. The asset is the **canonical source** — the spec §16 appendix references it by path, does not duplicate the text. When the guide evolves, only the asset is edited. [Codex FIX #14]
16. **No template versioning, no soft delete.** `template rm --confirm` hard-deletes the row. Phase 5 (broadcasts) references templates by id — if a broadcast was already sent with a template that gets deleted, the stored `broadcast.template_id` becomes a dangling reference that `broadcast show` surfaces as "template deleted (id=N)". Simpler than maintaining a soft-delete flag.
17. **Lint rule list is fixed** (see Task 4). Phase 4 does not ship a plugin system for custom lint rules. Operators who want more checks can wrap `template lint` in a shell script.
18. **Forbidden tags** the linter rejects as errors: `<script>`, `<form>`, `<iframe>`, `<object>`, `<embed>`, `<mj-include>`. Raw `<table>` and `<div>` outside `<mj-raw>` are also errors (not warnings — the entire point of MJML is to stop hand-written table hacking). [Codex FIX #6, ADD #8]
19. **Image + button validation:** `<mj-image>` must have an `alt` attribute (error if missing). `<mj-button>` must have a non-empty, non-`#` `href` attribute (error if missing or `#`). [Codex ADD #8, Gemini #5]
20. **CSS inlining runs once per compile call.** Phase 4 does not split inline-then-substitute (the Phase 5 perf optimization Gemini flagged). Phase 5's broadcast send path MAY inline the template once and then handlebars-substitute per-recipient — tracked as a Phase 5 design decision, not blocking Phase 4. [Gemini #2]

---

## File structure

Created by this phase:

```
mailing-list-cli/
├── assets/
│   └── template-authoring.md          # spec §16 text, bundled via include_str!
├── src/
│   ├── template/
│   │   ├── mod.rs                     # public API: parse(), compile(), lint()
│   │   ├── frontmatter.rs             # YAML frontmatter + body split via gray_matter
│   │   ├── compile.rs                 # Handlebars → mrml → css-inline → html2text
│   │   └── lint.rs                    # rule set + findings struct
│   └── commands/
│       └── template.rs                # CLI dispatch for all template subcommands
```

Modified:

```
├── Cargo.toml                         # +mrml, +handlebars, +css-inline, +html2text, +gray_matter
├── src/
│   ├── cli.rs                         # +Template subcommand + TemplateAction enum
│   ├── main.rs                        # dispatch Template command + mod template
│   ├── models.rs                      # +Template struct
│   ├── db/mod.rs                      # +template_* helpers
│   └── commands/
│       ├── mod.rs                     # +pub mod template
│       └── agent_info.rs              # advertise template commands + v0.1.0 status
└── tests/
    └── cli.rs                         # new integration tests per command
```

---

## Task 1: Add template dependencies + authoring guide asset

**Files:**
- Modify: `Cargo.toml`
- Create: `assets/template-authoring.md`

- [ ] **Step 1: Add the five new deps**

Edit `Cargo.toml`'s `[dependencies]` section (keep alphabetical). **Note: no `gray_matter` — we parse frontmatter by hand with `serde_yaml` to get deterministic errors and fewer transitive deps.**

```toml
[dependencies]
anyhow = "1.0"
chrono = { version = "0.4", features = ["clock", "serde"] }
clap = { version = "4.5", features = ["derive", "env"] }
css-inline = { version = "0.20", default-features = false, features = ["stylesheet-cache"] }
csv = "1.4"
dirs = "6.0"
handlebars = "6.4"
html2text = "0.16"
mrml = "5.1"
pest = "2.8"
pest_derive = "2.8"
rusqlite = { version = "0.37", features = ["bundled", "chrono"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
tempfile = "3.10"
thiserror = "2.0"
toml = "0.8"
```

**Note:** `tempfile` was previously dev-only but moves to the main `[dependencies]` section because `template edit` needs to write the template to a temp file before launching `$EDITOR`. If Cargo complains about duplicate entries, remove it from `[dev-dependencies]`.

The `css-inline` feature set is conservative (no default `cli` or `file` features) to keep the binary small.

- [ ] **Step 2: Create the authoring guide asset**

Create `assets/template-authoring.md` with the verbatim content of the spec's §16 Appendix (lines 1099-1251 of `docs/specs/2026-04-07-mailing-list-cli-design.md`). Use the Read tool on the spec file at `offset=1099 limit=153`, then Write the extracted markdown to `assets/template-authoring.md`. **Do not paraphrase or reword anything — this guide is the canonical authoring reference.**

- [ ] **Step 3: Verify deps resolve**

Run: `cargo build 2>&1 | tail -20`
Expected: successful build. Five new crates in `Cargo.lock`. No compile errors because nothing uses them yet.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock assets/template-authoring.md
git commit -m "feat(deps): add mrml, handlebars, css-inline, html2text, serde_yaml + authoring guide asset"
```

---

## Task 2: Template module skeleton

**Files:**
- Create: `src/template/mod.rs`
- Create: `src/template/frontmatter.rs`
- Create: `src/template/compile.rs` (stub)
- Create: `src/template/lint.rs` (stub)
- Modify: `src/main.rs` (add `mod template;`)

- [ ] **Step 1: Create the module root**

Create `src/template/mod.rs`:

```rust
//! Template subsystem: YAML-frontmatter MJML with Handlebars merge tags.
//!
//! Pipeline:
//!
//!   source   -->   [frontmatter::split]  -->  (VarSchema, body)
//!                                                  |
//!                                                  v
//!                                     [handlebars render + mrml compile]
//!                                                  |
//!                                                  v
//!                                         [css-inline + html2text]
//!                                                  |
//!                                                  v
//!                                             Rendered { html, text, subject }
//!
//! The compile module is pure: no DB access, no IO beyond the inputs. The
//! command layer (`src/commands/template.rs`) handles persistence, config
//! loading, and $EDITOR integration.

pub mod compile;
pub mod frontmatter;
pub mod lint;

pub use compile::{Rendered, compile, compile_with_placeholders, CompileError};
pub use frontmatter::{FrontmatterError, ParsedTemplate, VarSchema, Variable, split_frontmatter};
pub use lint::{LintFinding, LintOutcome, LintRule, Severity, lint};
```

- [ ] **Step 2: Create the frontmatter module**

Create `src/template/frontmatter.rs`:

```rust
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
    pub name: String,
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

    let schema: VarSchema = serde_yaml::from_str(yaml_block)
        .map_err(|e| FrontmatterError::Yaml(e.to_string()))?;

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
```

- [ ] **Step 3: Create the compile module (stub)**

Create `src/template/compile.rs`:

```rust
//! Template compile pipeline. Full implementation lands in Task 3.

use crate::template::frontmatter::{FrontmatterError, ParsedTemplate};
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
pub fn compile_with_placeholders(
    _source: &str,
    _data: &Value,
) -> Result<Rendered, CompileError> {
    // Implemented in Task 3.
    Err(CompileError::Handlebars("not yet implemented".into()))
}
```

- [ ] **Step 4: Create the lint module (stub)**

Create `src/template/lint.rs`:

```rust
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
```

- [ ] **Step 5: Wire the module**

Edit `src/main.rs` to add `mod template;` alphabetically after `mod segment;`:

```rust
mod cli;
mod commands;
mod config;
mod csv_import;
mod db;
mod email_cli;
mod error;
mod models;
mod output;
mod paths;
mod segment;
mod template;
```

- [ ] **Step 6: Build + run frontmatter tests**

Run: `cargo build 2>&1 | tail -10` — expect clean.
Run: `cargo test template::frontmatter 2>&1 | tail -10` — expect 4 passed.
Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -5` — expect clean.

- [ ] **Step 7: Commit**

```bash
git add src/template/ src/main.rs
git commit -m "feat(template): module skeleton + YAML frontmatter parser"
```

---

## Task 3: Compile pipeline — Handlebars → mrml → css-inline → html2text

**Files:**
- Modify: `src/template/compile.rs`

- [ ] **Step 1: Implement the full compile function**

Replace `src/template/compile.rs` with:

```rust
//! Template compile pipeline.
//!
//! source → frontmatter split → Handlebars render (with strict_mode)
//!        → mrml parse + render → css-inline → html2text → Rendered.

use crate::template::frontmatter::{ParsedTemplate, FrontmatterError, split_frontmatter};
use handlebars::Handlebars;
use serde::Serialize;
use serde_json::{Value, json};

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

const PLACEHOLDER_UNSUBSCRIBE: &str = "https://example.invalid/u/PLACEHOLDER_UNSUBSCRIBE_TOKEN";
const PLACEHOLDER_ADDRESS: &str =
    "Your Company Name · 123 Example Street · City, ST 00000";

/// Compile a template with merge data. Send-time-only placeholders
/// (`{{{ unsubscribe_link }}}`, `{{{ physical_address_footer }}}`) pass through
/// as literal text — they are substituted by the Phase 5 send pipeline.
pub fn compile(source: &str, data: &Value) -> Result<Rendered, CompileError> {
    compile_impl(source, data, false)
}

/// Same as `compile`, but also substitutes placeholder stub values for the two
/// send-time merge tags. Used by `template render --with-placeholders` for
/// agent-facing preview of the fully-rendered output.
pub fn compile_with_placeholders(source: &str, data: &Value) -> Result<Rendered, CompileError> {
    compile_impl(source, data, true)
}

fn compile_impl(
    source: &str,
    data: &Value,
    substitute_placeholders: bool,
) -> Result<Rendered, CompileError> {
    let ParsedTemplate { schema, body } = split_frontmatter(source)?;

    // Check required variables up-front so the error names the missing fields.
    for var in &schema.variables {
        if var.required {
            let present = data.get(&var.name).map_or(false, |v| !v.is_null());
            if !present {
                return Err(CompileError::MissingVariable(var.name.clone()));
            }
        }
    }

    // Augment data with placeholder stubs if requested.
    let mut effective = data.clone();
    if substitute_placeholders {
        if let Value::Object(map) = &mut effective {
            map.entry("unsubscribe_link".to_string())
                .or_insert(json!(PLACEHOLDER_UNSUBSCRIBE));
            map.entry("physical_address_footer".to_string())
                .or_insert(json!(PLACEHOLDER_ADDRESS));
        }
    }

    // Handlebars with strict mode — undeclared vars error instead of silently
    // rendering as empty string.
    let mut hb = Handlebars::new();
    hb.set_strict_mode(!substitute_placeholders);
    // When substituting placeholders, we allow missing vars because preview
    // data is intentionally incomplete; otherwise we're strict.
    if substitute_placeholders {
        hb.set_strict_mode(false);
    }

    // Render subject first.
    let subject = hb
        .render_template(&schema.subject, &effective)
        .map_err(|e| CompileError::Handlebars(format!("subject: {e}")))?;

    // Render body (still MJML at this point).
    let rendered_mjml = hb
        .render_template(&body, &effective)
        .map_err(|e| CompileError::Handlebars(format!("body: {e}")))?;

    // Parse + render MJML → HTML.
    let parsed = mrml::parse(&rendered_mjml)
        .map_err(|e| CompileError::Mjml(format!("{e}")))?;
    let render_opts = mrml::prelude::render::RenderOptions::default();
    let html = parsed
        .render(&render_opts)
        .map_err(|e| CompileError::Mjml(format!("render: {e}")))?;

    // Inline CSS for Outlook. Use a conservative InlineOptions.
    let inliner = css_inline::CSSInliner::options()
        .inline_style_tags(true)
        .keep_style_tags(false)
        .build();
    let inlined = inliner
        .inline(&html)
        .map_err(|e| CompileError::Inline(format!("{e}")))?;

    // Plain-text alternative via html2text.
    let text = html2text::from_read(inlined.as_bytes(), 80)
        .unwrap_or_else(|_| String::from("(plain-text render failed)"));

    let size_bytes = inlined.len();

    Ok(Rendered {
        subject,
        html: inlined,
        text,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MINIMAL: &str = r#"---
name: welcome
subject: "Welcome, {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Welcome</mj-title>
    <mj-preview>Confirm your email</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hi {{ first_name }}</mj-text>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

    #[test]
    fn compiles_minimal_template() {
        let rendered = compile(MINIMAL, &json!({ "first_name": "Alice" })).unwrap();
        assert_eq!(rendered.subject, "Welcome, Alice");
        assert!(rendered.html.contains("Hi Alice"));
        assert!(rendered.html.contains("{{{ unsubscribe_link }}}"));
        assert!(!rendered.text.is_empty());
        assert!(rendered.size_bytes > 0);
    }

    #[test]
    fn missing_required_variable_errors() {
        let err = compile(MINIMAL, &json!({})).unwrap_err();
        match err {
            CompileError::MissingVariable(name) => assert_eq!(name, "first_name"),
            _ => panic!("expected MissingVariable, got {err:?}"),
        }
    }

    #[test]
    fn compile_with_placeholders_substitutes_stubs() {
        let rendered = compile_with_placeholders(MINIMAL, &json!({ "first_name": "Alice" }))
            .unwrap();
        assert!(rendered.html.contains("PLACEHOLDER_UNSUBSCRIBE_TOKEN"));
        assert!(rendered.html.contains("Your Company Name"));
        // The raw triple-brace tokens should be GONE after substitution.
        assert!(!rendered.html.contains("{{{ unsubscribe_link }}}"));
    }

    #[test]
    fn rejects_invalid_mjml() {
        let bad = r#"---
name: bad
subject: "Hi"
---
<mjml><mj-body><BOGUS_TAG></BOGUS_TAG></mj-body></mjml>
"#;
        let err = compile(bad, &json!({})).unwrap_err();
        // mrml returns a parse error on unknown tags in strict mode.
        match err {
            CompileError::Mjml(_) => {}
            other => panic!("expected Mjml, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test template::compile 2>&1 | tail -20`
Expected: 4 passed.

If the `rejects_invalid_mjml` test fails because mrml treats unknown tags as warnings not errors, change the assertion to accept `CompileError::Mjml` OR substitute a different guaranteed-bad input (e.g. `<mjml><mj-body><mj-button>unclosed`).

- [ ] **Step 3: Run clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -5`
Run: `cargo fmt --check`

Both should be clean.

- [ ] **Step 4: Commit**

```bash
git add src/template/compile.rs
git commit -m "feat(template): compile pipeline (handlebars + mrml + css-inline + html2text)"
```

---

## Task 4: Lint rule set

**Files:**
- Modify: `src/template/lint.rs`

- [ ] **Step 1: Implement the full lint function**

Replace `src/template/lint.rs` with:

```rust
//! Template lint rules. See spec §7.3 for the authoritative list.

use crate::template::compile::{Rendered, compile_with_placeholders};
use crate::template::frontmatter::{FrontmatterError, ParsedTemplate, split_frontmatter};
use serde::Serialize;
use serde_json::{Value, json};

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
    FrontmatterInvalid,
    MjmlParseFailed,
    UndeclaredVariable,
    UnusedVariable,         // now an ERROR, not warning
    DangerousCss,
    HtmlSizeWarning,        // 90 KB warn threshold
    HtmlSizeError,          // 102 KB Gmail clip error
    EmptyPlainText,
    SubjectTooLong,
    SubjectEmpty,
    UnsubscribeLinkMissing,
    PhysicalAddressFooterMissing,
    MjPreviewMissing,
    ForbiddenTag,           // <script>/<form>/<iframe>/<object>/<embed>/<mj-include>
    RawTableOutsideMjRaw,   // now ERROR, not warning
    ImageMissingAlt,        // <mj-image> without alt
    ButtonMissingHref,      // <mj-button> without real href
    ForbiddenTripleBrace,   // {{{ foo }}} where foo is not in allowlist
    ForbiddenHelper,        // {{#each}} or {{> partial}}
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

const GMAIL_CLIP_ERROR: usize = 102_000;    // Gmail clips at 102 KB; clipping hides unsubscribe link
const GMAIL_CLIP_WARN: usize = 90_000;      // Warn before we hit the cliff
const SUBJECT_MAX_LEN: usize = 70;          // Codex FIX #6: was 100, tightened

const ALLOWED_TRIPLE_BRACE: &[&str] = &["unsubscribe_link", "physical_address_footer"];
const FORBIDDEN_TAGS: &[&str] = &[
    "<script", "<form", "<iframe", "<object", "<embed", "<mj-include",
];
const BUILT_INS: &[&str] = &[
    "first_name", "last_name", "email", "current_year", "broadcast_id",
    "unsubscribe_link", "physical_address_footer",
];

pub fn lint(source: &str) -> LintOutcome {
    let mut findings: Vec<LintFinding> = Vec::new();

    // 1. Frontmatter
    let parsed = match split_frontmatter(source) {
        Ok(p) => p,
        Err(e) => {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: match e {
                    FrontmatterError::Missing => LintRule::FrontmatterMissing,
                    _ => LintRule::FrontmatterInvalid,
                },
                message: format!("{e}"),
                hint: "Every template must start with a `---` delimited YAML frontmatter block declaring name, subject, and variables".into(),
            });
            return summarize(findings);
        }
    };
    let ParsedTemplate { schema, body } = parsed;

    // 2. Required presence checks on the body text
    if !body.contains("{{{ unsubscribe_link }}}") {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::UnsubscribeLinkMissing,
            message: "template body does not contain {{{ unsubscribe_link }}}".into(),
            hint: "Insert `{{{ unsubscribe_link }}}` inside an <mj-text> near the footer — it's replaced at send time with a one-click unsubscribe URL".into(),
        });
    }
    if !body.contains("{{{ physical_address_footer }}}") {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::PhysicalAddressFooterMissing,
            message: "template body does not contain {{{ physical_address_footer }}}".into(),
            hint: "Insert `{{{ physical_address_footer }}}` inside an <mj-text> near the unsubscribe link — required by CAN-SPAM".into(),
        });
    }

    // 3. Subject
    if schema.subject.is_empty() {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::SubjectEmpty,
            message: "subject is empty".into(),
            hint: "Add a subject line to the frontmatter".into(),
        });
    } else if schema.subject.len() > SUBJECT_MAX_LEN {
        findings.push(LintFinding {
            severity: Severity::Warning,
            rule: LintRule::SubjectTooLong,
            message: format!(
                "subject is {} chars (max recommended: {SUBJECT_MAX_LEN})",
                schema.subject.len()
            ),
            hint: "Long subjects are truncated on mobile — aim for < 50 chars when possible".into(),
        });
    }

    // 4. Mj-preview recommended (warning only)
    if !body.contains("<mj-preview>") {
        findings.push(LintFinding {
            severity: Severity::Warning,
            rule: LintRule::MjPreviewMissing,
            message: "template has no <mj-preview>".into(),
            hint: "Preview text in the inbox row dramatically affects open rates — add `<mj-preview>...</mj-preview>` to <mj-head>".into(),
        });
    }

    // 5. Dangerous CSS
    for pat in ["flex", "grid", "float:", "position:"] {
        if body.contains(pat) {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::DangerousCss,
                message: format!("body contains `{pat}` which breaks in Outlook desktop"),
                hint: "Use <mj-section>/<mj-column>/<mj-spacer> for layout instead of modern CSS".into(),
            });
        }
    }

    // 6. Forbidden tags (script/form/iframe/object/embed/mj-include)
    for tag in FORBIDDEN_TAGS {
        if body.contains(tag) {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ForbiddenTag,
                message: format!("template contains forbidden tag `{tag}`"),
                hint: "This tag is blocked by most email clients and/or by mailing-list-cli's security policy. Remove it.".into(),
            });
        }
    }

    // 7. Raw <table> or <div> outside <mj-raw> (now an error per Codex review)
    if body.contains("<table")
        && !body.contains("<mj-raw>")
        && !body.contains("<mj-table>")
    {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::RawTableOutsideMjRaw,
            message: "template contains raw `<table>` outside of `<mj-raw>` or `<mj-table>`".into(),
            hint: "Use `<mj-section>/<mj-column>` for layout or `<mj-table>` for tabular data".into(),
        });
    }

    // 8. <mj-image> must have alt attribute
    for idx in find_tag_positions(&body, "<mj-image") {
        let tag_end = body[idx..].find('>').map(|n| idx + n).unwrap_or(body.len());
        let tag_slice = &body[idx..tag_end];
        if !tag_slice.contains("alt=") {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ImageMissingAlt,
                message: "`<mj-image>` missing `alt` attribute".into(),
                hint: "Add `alt=\"descriptive text\"` to every image. Screen readers and spam filters care.".into(),
            });
            break; // report once, not per image
        }
    }

    // 9. <mj-button> must have non-empty, non-`#` href
    for idx in find_tag_positions(&body, "<mj-button") {
        let tag_end = body[idx..].find('>').map(|n| idx + n).unwrap_or(body.len());
        let tag_slice = &body[idx..tag_end];
        let href = extract_attr(tag_slice, "href");
        if href.as_deref().is_none_or(|h| h.is_empty() || h == "#") {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ButtonMissingHref,
                message: "`<mj-button>` missing real href (empty or `#`)".into(),
                hint: "Every button must have a real target URL or a merge tag like `{{ cta_url }}`.".into(),
            });
            break;
        }
    }

    // 10. Triple-brace allowlist
    for captured in extract_triple_brace_names(&body) {
        if !ALLOWED_TRIPLE_BRACE.contains(&captured.as_str()) {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::ForbiddenTripleBrace,
                message: format!(
                    "`{{{{{{ {captured} }}}}}}` uses triple-brace (raw HTML) but is not in the allowlist"
                ),
                hint: "Triple-brace is reserved for `unsubscribe_link` and `physical_address_footer`. Use double-brace `{{ name }}` for contact fields.".into(),
            });
        }
    }

    // 11. Forbidden helpers: {{#each}}, {{> partial}}
    if body.contains("{{#each") || body.contains("{{> ") {
        findings.push(LintFinding {
            severity: Severity::Error,
            rule: LintRule::ForbiddenHelper,
            message: "template uses `{{#each}}` or `{{> partial}}` which are not supported in v0.1".into(),
            hint: "Phase 4 supports scalar variables + `{{#if}}` / `{{#unless}}` only. Loops and partials land in v0.2+.".into(),
        });
    }

    // 12. Declared-vs-used variable check
    //    - Declared but not used in body or subject → ERROR (Codex FIX #6)
    //    - Used but not declared (and not in built-ins) → ERROR
    for var in &schema.variables {
        let tag2 = format!("{{{{ {} }}}}", var.name);
        let tag3 = format!("{{{{{{ {} }}}}}}", var.name);
        let tag2_sub = schema.subject.contains(&tag2);
        let tag_in_body = body.contains(&tag2) || body.contains(&tag3);
        if !tag2_sub && !tag_in_body {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::UnusedVariable,
                message: format!("variable `{}` is declared but never used", var.name),
                hint: format!("Either remove `{}` from the frontmatter or reference it in the body/subject", var.name),
            });
        }
    }

    // Scan body + subject for `{{ identifier }}` tokens and verify each is declared or built-in.
    let mut used = extract_merge_tag_names(&body);
    used.extend(extract_merge_tag_names(&schema.subject));
    for captured in used {
        let declared = schema.variables.iter().any(|v| v.name == captured);
        let built_in = BUILT_INS.contains(&captured.as_str());
        if !declared && !built_in {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::UndeclaredVariable,
                message: format!("variable `{captured}` is used but not declared in frontmatter"),
                hint: format!(
                    "Add `- name: {captured}\\n    type: string\\n    required: false` to `variables:` or use one of the built-ins: {}",
                    BUILT_INS.join(", ")
                ),
            });
        }
    }

    // 13. Compile + measure size.
    //    Use `compile_with_placeholders` so the send-time placeholders get stub
    //    values and we can measure the realistic post-inline HTML size.
    let stub_data = stub_data_for_variables(&schema);
    match compile_with_placeholders(source, &stub_data) {
        Ok(Rendered { html, text, .. }) => {
            if html.len() >= GMAIL_CLIP_ERROR {
                findings.push(LintFinding {
                    severity: Severity::Error,
                    rule: LintRule::HtmlSizeError,
                    message: format!(
                        "post-inline HTML is {} bytes (Gmail clips at {} bytes — the footer and unsubscribe link will be hidden)",
                        html.len(), GMAIL_CLIP_ERROR
                    ),
                    hint: "Reduce the template size — inline smaller images, remove redundant sections, or move content to a landing page. A clipped footer is a compliance failure, not just an aesthetic problem.".into(),
                });
            } else if html.len() >= GMAIL_CLIP_WARN {
                findings.push(LintFinding {
                    severity: Severity::Warning,
                    rule: LintRule::HtmlSizeWarning,
                    message: format!(
                        "post-inline HTML is {} bytes (Gmail clips at {} bytes — you're close to the cliff)",
                        html.len(), GMAIL_CLIP_ERROR
                    ),
                    hint: "Consider trimming the template before it grows past the clip limit.".into(),
                });
            }
            if text.trim().is_empty() {
                findings.push(LintFinding {
                    severity: Severity::Error,
                    rule: LintRule::EmptyPlainText,
                    message: "plain-text alternative is empty".into(),
                    hint: "html2text failed to extract readable text — ensure the template has actual <mj-text> content".into(),
                });
            }
        }
        Err(e) => {
            findings.push(LintFinding {
                severity: Severity::Error,
                rule: LintRule::MjmlParseFailed,
                message: format!("compile failed: {e}"),
                hint: "Run `template render <name>` to see the full compile error".into(),
            });
        }
    }

    summarize(findings)
}

/// Find all byte positions where `needle` appears in `haystack` (non-overlapping).
fn find_tag_positions(haystack: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(idx) = haystack[start..].find(needle) {
        out.push(start + idx);
        start += idx + needle.len();
    }
    out
}

/// Extract the value of `attr="value"` from a tag slice. Returns `None` if
/// the attribute is missing or malformed.
fn extract_attr(tag_slice: &str, attr: &str) -> Option<String> {
    let pat = format!("{attr}=\"");
    let idx = tag_slice.find(&pat)? + pat.len();
    let end = tag_slice[idx..].find('"')?;
    Some(tag_slice[idx..idx + end].to_string())
}

/// Extract `{{{ name }}}` identifiers (triple-brace).
fn extract_triple_brace_names(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search_start = 0;
    while let Some(rel) = body[search_start..].find("{{{") {
        let open = search_start + rel + 3;
        if let Some(close_rel) = body[open..].find("}}}") {
            let inner = body[open..open + close_rel].trim();
            if !inner.starts_with('#') && !inner.starts_with('/') {
                let name: String = inner
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    out.push(name);
                }
            }
            search_start = open + close_rel + 3;
        } else {
            break;
        }
    }
    out
}

fn stub_data_for_variables(schema: &crate::template::frontmatter::VarSchema) -> Value {
    let mut map = serde_json::Map::new();
    for var in &schema.variables {
        let v = match var.ty.as_str() {
            "number" => json!(0),
            "bool" => json!(false),
            _ => json!("stub"),
        };
        map.insert(var.name.clone(), v);
    }
    // Built-ins that the compile step expects present in non-strict mode.
    map.insert("first_name".into(), json!("stub"));
    map.insert("last_name".into(), json!("stub"));
    map.insert("email".into(), json!("stub@example.invalid"));
    map.insert("current_year".into(), json!(2026));
    map.insert("broadcast_id".into(), json!(0));
    Value::Object(map)
}

/// Extract `{{ name }}` and `{{{ name }}}` identifiers from the template body.
fn extract_merge_tag_names(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Skip a potential third `{`
            let start = if i + 2 < bytes.len() && bytes[i + 2] == b'{' {
                i + 3
            } else {
                i + 2
            };
            // Find the closing `}}`
            if let Some(close_rel) = body[start..].find("}}") {
                let inner = body[start..start + close_rel].trim();
                // skip helpers like `{{#if ...}}` — they start with #
                if !inner.starts_with('#') && !inner.starts_with('/') && !inner.is_empty() {
                    // An identifier, possibly with leading triple-brace trimmed already
                    let name: String = inner
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.push(name);
                    }
                }
                i = start + close_rel + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn summarize(findings: Vec<LintFinding>) -> LintOutcome {
    let error_count = findings.iter().filter(|f| f.severity == Severity::Error).count();
    let warning_count = findings.iter().filter(|f| f.severity == Severity::Warning).count();
    LintOutcome {
        findings,
        error_count,
        warning_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"---
name: welcome
subject: "Welcome, {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Welcome</mj-title>
    <mj-preview>Welcome, new friend</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hi {{ first_name }}</mj-text>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

    #[test]
    fn lints_clean_template() {
        let outcome = lint(GOOD);
        assert_eq!(outcome.error_count, 0, "findings: {:?}", outcome.findings);
    }

    #[test]
    fn flags_missing_unsubscribe_link() {
        let src = GOOD.replace("{{{ unsubscribe_link }}}", "");
        let outcome = lint(&src);
        assert!(outcome.has_errors());
        assert!(outcome
            .findings
            .iter()
            .any(|f| f.rule == LintRule::UnsubscribeLinkMissing));
    }

    #[test]
    fn flags_missing_physical_address_footer() {
        let src = GOOD.replace("{{{ physical_address_footer }}}", "");
        let outcome = lint(&src);
        assert!(outcome.has_errors());
        assert!(outcome
            .findings
            .iter()
            .any(|f| f.rule == LintRule::PhysicalAddressFooterMissing));
    }

    #[test]
    fn flags_dangerous_css() {
        let src = GOOD.replace("<mj-text>", "<mj-text css-class=\"display:flex;\">");
        let outcome = lint(&src);
        assert!(outcome
            .findings
            .iter()
            .any(|f| f.rule == LintRule::DangerousCss));
    }

    #[test]
    fn flags_unused_variable() {
        let src = GOOD.replace("variables:\n  - name: first_name", "variables:\n  - name: first_name\n    type: string\n    required: false\n  - name: unused_var");
        let outcome = lint(&src);
        assert!(outcome
            .findings
            .iter()
            .any(|f| f.rule == LintRule::UnusedVariable));
    }

    #[test]
    fn flags_missing_mj_preview_as_warning() {
        let src = GOOD.replace("<mj-preview>Welcome, new friend</mj-preview>", "");
        let outcome = lint(&src);
        assert!(outcome
            .findings
            .iter()
            .any(|f| f.rule == LintRule::MjPreviewMissing));
    }

    #[test]
    fn flags_subject_too_long_as_warning() {
        let long_subject = "x".repeat(120);
        let src = GOOD.replace(
            "Welcome, {{ first_name }}",
            &long_subject,
        );
        let outcome = lint(&src);
        assert!(outcome
            .findings
            .iter()
            .any(|f| f.rule == LintRule::SubjectTooLong));
    }
}
```

- [ ] **Step 2: Run the lint tests**

Run: `cargo test template::lint 2>&1 | tail -15`
Expected: 7 passed.

- [ ] **Step 3: Run clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings`
Run: `cargo fmt --check`

- [ ] **Step 4: Commit**

```bash
git add src/template/lint.rs
git commit -m "feat(template): lint rule set with 13 rules"
```

---

## Task 5: Template DB helpers

**Files:**
- Modify: `src/models.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Add the `Template` model struct**

Append to `src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub subject: String,
    pub mjml_source: String,
    pub schema_json: String,
    pub created_at: String,
    pub updated_at: String,
}
```

- [ ] **Step 2: Add `template_*` DB helpers**

Add to `impl Db` in `src/db/mod.rs` (after the segment helpers):

```rust
    // ─── Template operations ───────────────────────────────────────────

    pub fn template_upsert(
        &self,
        name: &str,
        subject: &str,
        mjml_source: &str,
        schema_json: &str,
    ) -> Result<i64, AppError> {
        if !is_snake_case(name) {
            return Err(AppError::BadInput {
                code: "invalid_template_name".into(),
                message: format!("template name '{name}' must be snake_case"),
                suggestion: "Use lowercase letters, digits, and underscores only (e.g. `welcome_email`)".into(),
            });
        }
        let now = chrono::Utc::now().to_rfc3339();
        // If a template with this name exists, UPDATE; else INSERT.
        let existing = self.template_get_by_name(name)?;
        if let Some(t) = existing {
            self.conn
                .execute(
                    "UPDATE template SET subject = ?1, mjml_source = ?2, schema_json = ?3, updated_at = ?4 WHERE id = ?5",
                    params![subject, mjml_source, schema_json, now, t.id],
                )
                .map_err(query_err)?;
            Ok(t.id)
        } else {
            self.conn
                .execute(
                    "INSERT INTO template (name, subject, mjml_source, schema_json, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    params![name, subject, mjml_source, schema_json, now],
                )
                .map_err(query_err)?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    pub fn template_all(&self) -> Result<Vec<crate::models::Template>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, subject, mjml_source, schema_json, created_at, updated_at
                 FROM template ORDER BY name ASC",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::Template {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    subject: row.get(2)?,
                    mjml_source: row.get(3)?,
                    schema_json: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
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
            "SELECT id, name, subject, mjml_source, schema_json, created_at, updated_at
             FROM template WHERE name = ?1",
            params![name],
            |row| {
                Ok(crate::models::Template {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    subject: row.get(2)?,
                    mjml_source: row.get(3)?,
                    schema_json: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
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
```

- [ ] **Step 3: Add DB tests**

Append to the `tests` module in `src/db/mod.rs`:

```rust
    #[test]
    fn template_crud_round_trip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let id = db
            .template_upsert("welcome", "Hi", "<mjml></mjml>", "{}")
            .unwrap();
        assert!(id > 0);
        let fetched = db.template_get_by_name("welcome").unwrap().unwrap();
        assert_eq!(fetched.subject, "Hi");

        // Upsert updates the existing row
        let id2 = db
            .template_upsert("welcome", "Hello", "<mjml></mjml>", "{}")
            .unwrap();
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
        assert!(db.template_upsert("WelcomeEmail", "Hi", "<mjml></mjml>", "{}").is_err());
        assert!(db.template_upsert("welcome-email", "Hi", "<mjml></mjml>", "{}").is_err());
        assert!(db.template_upsert("welcome_email", "Hi", "<mjml></mjml>", "{}").is_ok());
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test db::tests::template 2>&1 | tail -10`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/models.rs src/db/mod.rs
git commit -m "feat(db): template_* helpers with upsert and snake_case validation"
```

---

## Task 6: `template create` + `template ls` + `template show` + `template rm`

**Files:**
- Modify: `src/cli.rs`
- Create: `src/commands/template.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add `Template` subcommand + action enum to CLI**

In `src/cli.rs`, add to `enum Command` (after `Segment`):

```rust
    /// Manage MJML templates (with the embedded agent authoring guide)
    Template {
        #[command(subcommand)]
        action: TemplateAction,
    },
```

At the bottom of the file:

```rust
#[derive(Subcommand, Debug)]
pub enum TemplateAction {
    /// Create a new template (scaffold) or import an existing MJML file
    Create(TemplateCreateArgs),
    /// List all templates
    #[command(visible_alias = "ls")]
    List,
    /// Print a template's MJML source
    Show(TemplateShowArgs),
    /// Render a template with merge data (returns JSON)
    Render(TemplateRenderArgs),
    /// Run the lint rule set against a template
    Lint(TemplateLintArgs),
    /// Open a template in $EDITOR (then re-lint and save)
    Edit(TemplateEditArgs),
    /// Delete a template
    Rm(TemplateRmArgs),
    /// Print the embedded agent authoring guide
    Guidelines,
}

#[derive(Args, Debug)]
pub struct TemplateCreateArgs {
    /// Template name (snake_case)
    pub name: String,
    /// Import MJML + frontmatter from this file path instead of scaffolding
    #[arg(long = "from-file")]
    pub from_file: Option<std::path::PathBuf>,
}

#[derive(Args, Debug)]
pub struct TemplateShowArgs {
    pub name: String,
}

#[derive(Args, Debug)]
pub struct TemplateRenderArgs {
    pub name: String,
    /// JSON file with merge data (an object: { "first_name": "Alice" })
    #[arg(long = "with-data")]
    pub with_data: Option<std::path::PathBuf>,
    /// Substitute placeholder stubs for unsubscribe_link and physical_address_footer
    #[arg(long = "with-placeholders")]
    pub with_placeholders: bool,
}

#[derive(Args, Debug)]
pub struct TemplateLintArgs {
    pub name: String,
}

#[derive(Args, Debug)]
pub struct TemplateEditArgs {
    pub name: String,
    /// Save even if the lint still has errors after the edit
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct TemplateRmArgs {
    pub name: String,
    #[arg(long)]
    pub confirm: bool,
}
```

- [ ] **Step 2: Create the command module**

Create `src/commands/template.rs`:

```rust
use crate::cli::{
    TemplateAction, TemplateCreateArgs, TemplateEditArgs, TemplateLintArgs, TemplateRenderArgs,
    TemplateRmArgs, TemplateShowArgs,
};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use crate::template::{
    FrontmatterError, ParsedTemplate, compile, compile_with_placeholders, lint, split_frontmatter,
};
use serde_json::{Value, json};

const AUTHORING_GUIDE: &str = include_str!("../../assets/template-authoring.md");

const SCAFFOLD: &str = r#"---
name: {{NAME}}
subject: "Your subject line"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Email title</mj-title>
    <mj-preview>Inbox preview text</mj-preview>
  </mj-head>
  <mj-body background-color="#f4f4f4">
    <mj-section background-color="#ffffff" padding="20px">
      <mj-column>
        <mj-text font-size="24px" font-weight="700">
          Hi {{ first_name }}
        </mj-text>
        <mj-text>
          Replace this body with your content. Remember to keep it under 600px wide.
        </mj-text>
        <mj-text font-size="12px" color="#666666">
          {{{ unsubscribe_link }}}
          <br/>
          {{{ physical_address_footer }}}
        </mj-text>
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

pub fn run(format: Format, action: TemplateAction) -> Result<(), AppError> {
    match action {
        TemplateAction::Create(args) => create(format, args),
        TemplateAction::List => list(format),
        TemplateAction::Show(args) => show(format, args),
        TemplateAction::Render(args) => render(format, args),
        TemplateAction::Lint(args) => lint_cmd(format, args),
        TemplateAction::Edit(args) => edit(format, args),
        TemplateAction::Rm(args) => remove(format, args),
        TemplateAction::Guidelines => guidelines(format),
    }
}

fn guidelines(format: Format) -> Result<(), AppError> {
    // The guidelines command prints raw markdown to stdout in human mode and
    // wraps it in a JSON envelope in --json mode so agents can grep for lines.
    match format {
        Format::Json => {
            output::success(
                Format::Json,
                "authoring guide",
                json!({ "guide_markdown": AUTHORING_GUIDE }),
            );
        }
        Format::Human => {
            println!("{AUTHORING_GUIDE}");
        }
    }
    Ok(())
}

fn create(format: Format, args: TemplateCreateArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let source = match args.from_file {
        Some(path) => std::fs::read_to_string(&path).map_err(|e| AppError::BadInput {
            code: "template_file_read_failed".into(),
            message: format!("could not read {}: {e}", path.display()),
            suggestion: "Check the file path and permissions".into(),
        })?,
        None => SCAFFOLD.replace("{{NAME}}", &args.name),
    };

    // Parse the frontmatter so we can persist schema_json + subject.
    let parsed = split_frontmatter(&source).map_err(frontmatter_to_bad_input)?;
    if parsed.schema.name != args.name {
        return Err(AppError::BadInput {
            code: "template_name_mismatch".into(),
            message: format!(
                "template file declares name '{}' but the CLI argument was '{}'",
                parsed.schema.name, args.name
            ),
            suggestion: "Make the frontmatter `name:` match the argument, or omit `name:` and let the CLI set it".into(),
        });
    }
    let schema_json = serde_json::to_string(&parsed.schema).unwrap();
    let id = db.template_upsert(&args.name, &parsed.schema.subject, &source, &schema_json)?;

    output::success(
        format,
        &format!("template created: {}", args.name),
        json!({
            "id": id,
            "name": args.name,
            "subject": parsed.schema.subject,
            "scaffolded": true
        }),
    );
    Ok(())
}

fn list(format: Format) -> Result<(), AppError> {
    let db = Db::open()?;
    let templates = db.template_all()?;
    let summary: Vec<_> = templates
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "subject": t.subject,
                "size_bytes": t.mjml_source.len(),
                "updated_at": t.updated_at
            })
        })
        .collect();
    let count = summary.len();
    output::success(
        format,
        &format!("{count} template(s)"),
        json!({ "templates": summary, "count": count }),
    );
    Ok(())
}

fn show(format: Format, args: TemplateShowArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls` to see all templates".into(),
        })?;
    output::success(
        format,
        &format!("template: {}", t.name),
        json!({
            "id": t.id,
            "name": t.name,
            "subject": t.subject,
            "mjml_source": t.mjml_source,
            "schema_json": t.schema_json,
            "updated_at": t.updated_at
        }),
    );
    Ok(())
}

fn render(format: Format, args: TemplateRenderArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;

    let data: Value = match &args.with_data {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| AppError::BadInput {
                code: "data_file_read_failed".into(),
                message: format!("could not read {}: {e}", path.display()),
                suggestion: "Check the file path and permissions".into(),
            })?;
            serde_json::from_str(&text).map_err(|e| AppError::BadInput {
                code: "data_file_invalid_json".into(),
                message: format!("{} is not valid JSON: {e}", path.display()),
                suggestion: "Provide a file containing a single JSON object".into(),
            })?
        }
        None => json!({}),
    };

    let rendered = if args.with_placeholders {
        compile_with_placeholders(&t.mjml_source, &data)
    } else {
        compile(&t.mjml_source, &data)
    }
    .map_err(compile_to_bad_input)?;

    let lint_outcome = lint(&t.mjml_source);
    output::success(
        format,
        &format!("rendered template '{}'", t.name),
        json!({
            "name": t.name,
            "subject": rendered.subject,
            "html": rendered.html,
            "text": rendered.text,
            "size_bytes": rendered.size_bytes,
            "lint_warnings": lint_outcome.warning_count,
            "lint_errors": lint_outcome.error_count
        }),
    );
    Ok(())
}

fn lint_cmd(format: Format, args: TemplateLintArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;
    let outcome = lint(&t.mjml_source);
    if outcome.has_errors() {
        return Err(AppError::BadInput {
            code: "template_lint_errors".into(),
            message: format!(
                "template '{}' has {} lint error(s)",
                t.name, outcome.error_count
            ),
            suggestion: serde_json::to_string(&outcome.findings).unwrap(),
        });
    }
    output::success(
        format,
        &format!(
            "lint passed with {} warning(s)",
            outcome.warning_count
        ),
        json!({
            "name": t.name,
            "errors": outcome.error_count,
            "warnings": outcome.warning_count,
            "findings": outcome.findings
        }),
    );
    Ok(())
}

fn edit(format: Format, args: TemplateEditArgs) -> Result<(), AppError> {
    // Guard: this is the ONLY interactive command. Refuse if we're not in a TTY,
    // if --json mode forced the envelope, or if $VISUAL/$EDITOR is unset.
    // No silent `vi` fallback — that's a footgun for remote/CI users.
    if format == Format::Json {
        return Err(AppError::BadInput {
            code: "edit_not_available_in_json_mode".into(),
            message: "template edit is the only interactive command and cannot run with --json".into(),
            suggestion: "Run without --json, or use `template create --from-file <path>` to update a template from a file on disk".into(),
        });
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return Err(AppError::BadInput {
            code: "edit_requires_tty".into(),
            message: "template edit requires an interactive TTY on stdout".into(),
            suggestion: "Use `template create --from-file <path>` when running from scripts or CI".into(),
        });
    }
    let editor_path = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .map_err(|_| AppError::Config {
            code: "editor_not_set".into(),
            message: "neither $VISUAL nor $EDITOR is set".into(),
            suggestion: "Set $EDITOR to a valid editor path, e.g. `export EDITOR=vim`".into(),
        })?;

    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;
    let tmpdir = tempfile::TempDir::new().map_err(|e| AppError::Transient {
        code: "tempfile_create_failed".into(),
        message: format!("could not create tempfile: {e}"),
        suggestion: "Check /tmp write permissions".into(),
    })?;
    let path = tmpdir.path().join(format!("{}.mjml.hbs", args.name));
    std::fs::write(&path, &t.mjml_source).map_err(|e| AppError::Transient {
        code: "tempfile_write_failed".into(),
        message: format!("could not write tempfile: {e}"),
        suggestion: "Check /tmp write permissions".into(),
    })?;
    // IMPORTANT: invoke the editor directly via Command::new, not via a shell.
    // This avoids command-injection risk if $EDITOR contains shell metacharacters.
    let status = std::process::Command::new(&editor_path)
        .arg(&path)
        .status()
        .map_err(|e| AppError::Config {
            code: "editor_launch_failed".into(),
            message: format!("could not launch editor ({editor_path}): {e}"),
            suggestion: "Set $EDITOR to a valid editor binary on PATH".into(),
        })?;
    if !status.success() {
        return Err(AppError::BadInput {
            code: "editor_exited_nonzero".into(),
            message: format!("editor {editor} exited with non-zero status"),
            suggestion: "Re-run `template edit` or edit the template with `template create --from-file`".into(),
        });
    }
    let new_source = std::fs::read_to_string(&path).map_err(|e| AppError::Transient {
        code: "tempfile_read_failed".into(),
        message: format!("could not read edited template: {e}"),
        suggestion: "Re-run `template edit`".into(),
    })?;
    let outcome = lint(&new_source);
    if outcome.has_errors() && !args.force {
        return Err(AppError::BadInput {
            code: "template_lint_errors".into(),
            message: format!(
                "edited template has {} lint error(s); NOT saved. Re-run with --force to save anyway",
                outcome.error_count
            ),
            suggestion: serde_json::to_string(&outcome.findings).unwrap(),
        });
    }
    let parsed = split_frontmatter(&new_source).map_err(frontmatter_to_bad_input)?;
    let schema_json = serde_json::to_string(&parsed.schema).unwrap();
    db.template_upsert(&args.name, &parsed.schema.subject, &new_source, &schema_json)?;
    output::success(
        format,
        &format!("template '{}' saved", args.name),
        json!({
            "name": args.name,
            "lint_errors": outcome.error_count,
            "lint_warnings": outcome.warning_count
        }),
    );
    Ok(())
}

fn remove(format: Format, args: TemplateRmArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("deleting template '{}' requires --confirm", args.name),
            suggestion: format!("rerun with `mailing-list-cli template rm {} --confirm`", args.name),
        });
    }
    let db = Db::open()?;
    if !db.template_delete(&args.name)? {
        return Err(AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        });
    }
    output::success(
        format,
        &format!("template '{}' removed", args.name),
        json!({ "name": args.name, "removed": true }),
    );
    Ok(())
}

// Helpers to turn internal errors into BadInput with agent-friendly messages.

fn frontmatter_to_bad_input(e: FrontmatterError) -> AppError {
    AppError::BadInput {
        code: "template_frontmatter_invalid".into(),
        message: format!("{e}"),
        suggestion: "Every template must start with `---`, declare `name`, `subject`, and optionally `variables`".into(),
    }
}

fn compile_to_bad_input(e: crate::template::CompileError) -> AppError {
    AppError::BadInput {
        code: "template_compile_failed".into(),
        message: format!("{e}"),
        suggestion: "Run `mailing-list-cli template lint <name>` for a detailed rule breakdown".into(),
    }
}
```

- [ ] **Step 3: Wire the module**

Edit `src/commands/mod.rs` to add `pub mod template;` (keep alphabetical):

```rust
pub mod agent_info;
pub mod contact;
pub mod field;
pub mod health;
pub mod list;
pub mod segment;
pub mod skill;
pub mod tag;
pub mod template;
pub mod update;
```

Edit `src/main.rs` to add the dispatch:

```rust
        Command::Template { action } => commands::template::run(format, action),
```

- [ ] **Step 4: Build + run the full test suite**

Run: `cargo build 2>&1 | tail -10`
Run: `cargo test -- --test-threads=1 2>&1 | grep "test result"`

Expected: all existing tests still pass (~101 plus new template ones).

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/commands/template.rs src/commands/mod.rs src/main.rs
git commit -m "feat(template): create/ls/show/render/lint/edit/rm/guidelines CLI"
```

---

## Task 7: Integration tests for template commands

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add integration tests**

Append to `tests/cli.rs`:

```rust
const VALID_TEMPLATE: &str = r#"---
name: welcome
subject: "Welcome, {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Welcome</mj-title>
    <mj-preview>Welcome to our list</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hi {{ first_name }}</mj-text>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

fn write_template_file(tmp: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = tmp.path().join(format!("{name}.mjml.hbs"));
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn template_create_from_file_list_show_round_trip() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);

    // Create from file
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "template", "create", "welcome", "--from-file",
            template_path.to_str().unwrap(),
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["name"], "welcome");
    assert_eq!(v["data"]["subject"], "Welcome, {{ first_name }}");

    // List
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);

    // Show
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "show", "welcome"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert!(v["data"]["mjml_source"].as_str().unwrap().contains("mjml"));
}

#[test]
fn template_create_scaffold_without_file() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "create", "scaffold"]);
    cmd.assert().success();
}

#[test]
fn template_render_with_data_returns_html() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "template", "create", "welcome", "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let data_path = tmp.path().join("data.json");
    std::fs::write(&data_path, r#"{"first_name":"Alice"}"#).unwrap();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "template", "render", "welcome", "--with-data",
            data_path.to_str().unwrap(),
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["subject"], "Welcome, Alice");
    assert!(v["data"]["html"].as_str().unwrap().contains("Hi Alice"));
    assert!(v["data"]["size_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn template_lint_clean_template_passes() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "template", "create", "welcome", "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "lint", "welcome"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["errors"], 0);
}

#[test]
fn template_lint_missing_unsubscribe_errors() {
    let (tmp, config_path, db_path) = stub_env();
    let bad = VALID_TEMPLATE.replace("{{{ unsubscribe_link }}}", "");
    let template_path = write_template_file(&tmp, "bad", &bad);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "template", "create", "bad", "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "lint", "bad"]);
    cmd.assert().failure().code(3);
}

#[test]
fn template_rm_without_confirm_fails() {
    let (tmp, config_path, db_path) = stub_env();
    let template_path = write_template_file(&tmp, "welcome", VALID_TEMPLATE);
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "template", "create", "welcome", "--from-file",
            template_path.to_str().unwrap(),
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "template", "rm", "welcome"]);
    cmd.assert().failure().code(3);
}

#[test]
fn template_guidelines_prints_authoring_guide() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.args(["--json", "template", "guidelines"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let guide = v["data"]["guide_markdown"].as_str().unwrap();
    assert!(guide.contains("Template Authoring for mailing-list-cli"));
    assert!(guide.contains("{{{ unsubscribe_link }}}"));
}
```

- [ ] **Step 2: Run the new integration tests**

Run: `cargo test template_ -- --test-threads=1 2>&1 | tail -15`
Expected: 7 passed.

Run the full suite as well:
`cargo test -- --test-threads=1 2>&1 | grep "test result"`

- [ ] **Step 3: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings`
Run: `cargo fmt --check`

- [ ] **Step 4: Commit**

```bash
git add tests/cli.rs
git commit -m "test(template): integration tests for create/ls/show/render/lint/rm/guidelines"
```

---

## Task 8: Update agent-info manifest + version bump + tag v0.1.0

**Files:**
- Modify: `src/commands/agent_info.rs`
- Modify: `Cargo.toml`
- Modify: `README.md` (badge update if present)

- [ ] **Step 1: Add template commands to agent-info**

In `src/commands/agent_info.rs`, add the following entries to the `commands` object (place them in logical order after the segment entries):

```rust
            "template create <name> [--from-file <path>]": "Create a new template (scaffold or import MJML file)",
            "template ls": "List all templates",
            "template show <name>": "Print a template's MJML source",
            "template render <name> [--with-data <file.json>] [--with-placeholders]": "Compile template → JSON { subject, html, text }",
            "template lint <name>": "Run the 13-rule lint set; exit 3 on errors",
            "template edit <name> [--force]": "Open in $EDITOR, re-lint on save",
            "template rm <name> --confirm": "Delete a template",
            "template guidelines": "Print the embedded agent authoring guide",
```

Update the `status` string:

```rust
        "status": "v0.1.0 — templates, MJML compile pipeline, lint, agent authoring guide"
```

- [ ] **Step 2: Add an agent-info test for the new commands**

Append to `tests/cli.rs`:

```rust
#[test]
fn agent_info_lists_phase_4_commands() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let commands = v["commands"].as_object().unwrap();
    for key in [
        "template create <name> [--from-file <path>]",
        "template render <name> [--with-data <file.json>] [--with-placeholders]",
        "template lint <name>",
        "template guidelines",
    ] {
        assert!(commands.contains_key(key), "agent-info missing {key}");
    }
    assert!(v["status"].as_str().unwrap().starts_with("v0.1.0"));
}
```

- [ ] **Step 3: Bump version**

Edit `Cargo.toml`:

```toml
version = "0.1.0"
```

- [ ] **Step 4: Update README badge**

```bash
grep -n "v0\.0\.[0-9]" README.md
```

If a match is found, edit the badge URL to `v0.1.0`. If no match, skip.

- [ ] **Step 5: Full verification sweep**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1 2>&1 | grep "test result"
```

All must be clean. Target tests: ≥ 75 unit + ≥ 45 integration (Phase 3 baseline 66+35, +~9 unit tests from frontmatter/compile/lint, +~8 integration tests from template commands).

- [ ] **Step 6: Commit + tag**

```bash
git add src/commands/agent_info.rs tests/cli.rs Cargo.toml Cargo.lock README.md
git commit -m "chore: bump to v0.1.0 — phase 4 templates"
git push origin main
git tag -a v0.1.0 -m "v0.1.0 — templates, MJML compile pipeline, lint, agent authoring guide"
git push origin v0.1.0
gh run list --repo paperfoot/mailing-list-cli --limit 1
```

---

## What Phase 4 does NOT ship

1. **`template edit` is the only interactive command** — justified by editor integration. No other template command prompts.
2. **No live preview server.** The spec calls for `template render` returning HTML; agents or humans can pipe it to a file and open it.
3. **No per-segment or per-broadcast template variants.** Templates are globally named, not scoped.
4. **No image CDN integration.** Operators embed image URLs directly in `<mj-image src="...">`.
5. **No Markdown-body-wrapped-in-MJML-shell hybrid.** Deferred to v0.2 per spec §7.4.
6. **No template versioning / history.** Deleting a template is destructive. Phase 5 broadcasts that reference a deleted template surface the dangling reference.
7. **No custom lint plugins.** The rule set is fixed.
8. **No Litmus / email-on-acid integration.** The linter catches the most common issues; cross-client verification is a manual step.

---

## Acceptance criteria

- [ ] All 8 tasks checked off.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test -- --test-threads=1` all clean.
- [ ] Baseline 101 tests + ≥ 17 new = ≥ 118 total.
- [ ] `template guidelines` prints the authoring guide verbatim.
- [ ] `template lint` on a clean template reports 0 errors. On a template missing either required placeholder, reports an error and exits 3.
- [ ] `template render` with valid merge data returns a JSON envelope with `subject`, `html`, `text`, `size_bytes`.
- [ ] `template create --from-file` successfully imports the `VALID_TEMPLATE` fixture.
- [ ] `Cargo.toml` is `0.1.0`, tag `v0.1.0` pushed, CI run triggered.
- [ ] `agent-info` advertises every new template command.

---

*End of Phase 4 implementation plan.*
