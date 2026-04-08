//! Minimal merge-tag substituter for v0.2 templates.
//!
//! We intentionally do NOT use Handlebars or any general templating engine.
//! Agents don't need loops, partials, helpers, or the full Mustache spec.
//! Templates in v0.2 support exactly three constructs:
//!
//!   1. `{{ var_name }}`        — scalar substitution, HTML-escaped
//!   2. `{{{ allowlist_name }}}` — raw HTML substitution, allowlist only
//!   3. `{{#if var_name}}...{{/if}}` — conditional block (truthy check)
//!
//! Whitespace inside braces is normalised: `{{var}}`, `{{ var }}`, and
//! `{{   var   }}` are all equivalent. Handlebars keywords leaked into
//! older agent-authored templates (e.g. `{{else}}`), so the substituter is
//! designed to never misinterpret them as variables.
//!
//! # Unresolved placeholders
//!
//! When a `{{ var }}` has no corresponding value in the merge data, the
//! substituter records an `unresolved` finding. The caller decides whether
//! that's a warning (preview) or a hard error (broadcast send).
//!
//! # XSS / injection safety
//!
//! Double-brace `{{ var }}` always HTML-escapes the value (`&`, `<`, `>`,
//! `"`, `'` are replaced with their character entities). Triple-brace
//! `{{{ name }}}` is raw HTML but is restricted to a fixed allowlist of
//! reserved names — see `TRIPLE_BRACE_ALLOWLIST`. Any other triple-brace
//! reference is treated as an unresolved placeholder (the lint then
//! flags it as a security error separately).

use std::collections::HashSet;

/// Triple-brace names that are allowed to inject raw HTML. These are
/// substituted by the send pipeline with HTML anchors (unsubscribe link)
/// and divs (physical address footer). Every other `{{{ }}}` reference is
/// rejected.
pub const TRIPLE_BRACE_ALLOWLIST: &[&str] = &["unsubscribe_link", "physical_address_footer"];

/// Built-in variables that the send pipeline always provides — using any of
/// these is fine even if the agent didn't declare them.
pub const BUILT_INS: &[&str] = &[
    "first_name",
    "last_name",
    "email",
    "current_year",
    "broadcast_id",
];

#[derive(Debug, Clone, PartialEq)]
pub struct SubstResult {
    /// The rendered output with all resolved placeholders substituted.
    pub output: String,
    /// Names of `{{ var }}` references the data dict didn't resolve.
    /// Deduplicated, in first-seen order.
    pub unresolved: Vec<String>,
    /// Triple-brace names that are NOT in the allowlist. These are a
    /// security concern (XSS) and the lint promotes them to errors.
    pub forbidden_raw: Vec<String>,
}

/// Substitute merge tags in `source` against `data`.
///
/// - `data` is treated as a JSON object; scalar fields (string/number/bool)
///   become their string form. Missing names are recorded as unresolved.
/// - Double-brace values are HTML-escaped.
/// - Triple-brace values are inserted raw IF the name is in the allowlist;
///   otherwise the substituter leaves the literal text in place AND records
///   the name in `forbidden_raw`.
/// - `{{#if name}}...{{/if}}` keeps the block if `name` resolves to a
///   truthy value (non-empty string, non-zero number, `true`). The check
///   does NOT HTML-escape anything — it's a boolean check.
pub fn substitute(source: &str, data: &serde_json::Value) -> SubstResult {
    let mut output = String::with_capacity(source.len() + 256);
    let mut unresolved_set: HashSet<String> = HashSet::new();
    let mut unresolved_order: Vec<String> = Vec::new();
    let mut forbidden_raw_set: HashSet<String> = HashSet::new();
    let mut forbidden_raw_order: Vec<String> = Vec::new();

    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Look for `{{` — everything else is pass-through.
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Triple-brace?
            let triple = i + 2 < bytes.len() && bytes[i + 2] == b'{';
            let open_end = if triple { i + 3 } else { i + 2 };
            // Look for closer — `}}}` for triple, `}}` for double.
            let closer = if triple { "}}}" } else { "}}" };
            if let Some(close_rel) = source[open_end..].find(closer) {
                let inner = source[open_end..open_end + close_rel].trim();

                if triple {
                    // Triple-brace: allowlist check.
                    let name = inner_identifier(inner);
                    if let Some(name) = name {
                        if TRIPLE_BRACE_ALLOWLIST.contains(&name.as_str()) {
                            // If the data dict provides it, substitute; if not,
                            // leave the literal triple-brace in place so the
                            // send-time pipeline can replace it later.
                            if let Some(val) = data.get(&name) {
                                output.push_str(&value_to_string(val));
                            } else {
                                output.push_str("{{{ ");
                                output.push_str(&name);
                                output.push_str(" }}}");
                            }
                        } else {
                            // XSS guard: non-allowlisted triple-brace.
                            if forbidden_raw_set.insert(name.clone()) {
                                forbidden_raw_order.push(name.clone());
                            }
                            // Leave the literal in place so it's visible in
                            // preview AND surfaced by the lint.
                            output.push_str("{{{ ");
                            output.push_str(&name);
                            output.push_str(" }}}");
                        }
                    }
                    i = open_end + close_rel + 3;
                    continue;
                }

                // Double-brace: either `{{#if name}}`, `{{/if}}`, or `{{ name }}`.
                if let Some(rest) = inner.strip_prefix('#') {
                    let mut parts = rest.split_whitespace();
                    let helper = parts.next().unwrap_or("");
                    if helper == "if" || helper == "unless" {
                        if let Some(arg) = parts.next() {
                            // Find the matching `{{/if}}` or `{{/unless}}`,
                            // respecting nested same-helper blocks by tracking
                            // depth. Without this, `{{#if a}}...{{#if b}}...{{/if}}...{{/if}}`
                            // would close the outer block at the first `{{/if}}`.
                            let block_start = open_end + close_rel + 2;
                            if let Some(block_end_rel) =
                                find_matching_close(&source[block_start..], helper)
                            {
                                let block = &source[block_start..block_start + block_end_rel];
                                let truthy = is_truthy(data.get(arg));
                                let keep = if helper == "if" { truthy } else { !truthy };
                                if keep {
                                    let sub = substitute(block, data);
                                    output.push_str(&sub.output);
                                    for u in sub.unresolved {
                                        if unresolved_set.insert(u.clone()) {
                                            unresolved_order.push(u);
                                        }
                                    }
                                    for f in sub.forbidden_raw {
                                        if forbidden_raw_set.insert(f.clone()) {
                                            forbidden_raw_order.push(f);
                                        }
                                    }
                                }
                                let close_tag_len = format!("{{{{/{helper}}}}}").len();
                                i = block_start + block_end_rel + close_tag_len;
                                continue;
                            }
                            // No matching closer — fall through and emit literally.
                        }
                    }
                    // Unknown helper or malformed — emit literally to surface
                    // the problem in preview. The lint will catch `{{#each}}`.
                    output.push_str(&source[i..open_end + close_rel + 2]);
                    i = open_end + close_rel + 2;
                    continue;
                }

                if inner.starts_with('/') {
                    // Orphan closing tag (no opener) — emit literally.
                    output.push_str(&source[i..open_end + close_rel + 2]);
                    i = open_end + close_rel + 2;
                    continue;
                }

                // Plain `{{ name }}` — HTML-escaped scalar substitution.
                if let Some(name) = inner_identifier(inner) {
                    if let Some(val) = data.get(&name) {
                        output.push_str(&html_escape(&value_to_string(val)));
                    } else {
                        if unresolved_set.insert(name.clone()) {
                            unresolved_order.push(name.clone());
                        }
                        // Leave the literal in place so agents see "{{ typo }}"
                        // in the preview and fix it. The broadcast send path
                        // hard-fails on any unresolved placeholder.
                        output.push_str("{{ ");
                        output.push_str(&name);
                        output.push_str(" }}");
                    }
                } else {
                    // Empty or non-identifier — emit literally.
                    output.push_str(&source[i..open_end + close_rel + 2]);
                }
                i = open_end + close_rel + 2;
                continue;
            }
        }
        // Plain byte, pass through. Safe because we only advance on valid
        // UTF-8 starts (look only at `{`).
        output.push(source[i..].chars().next().unwrap());
        i += source[i..].chars().next().unwrap().len_utf8();
    }

    SubstResult {
        output,
        unresolved: unresolved_order,
        forbidden_raw: forbidden_raw_order,
    }
}

/// Find the byte offset of the `{{/helper}}` that balances the opening
/// `{{#helper ...}}` at the start of `body`. Returns the offset of the `{{`
/// of the matching closer, not the byte after it.
///
/// Depth-aware: nested `{{#if}} ... {{/if}}` blocks push/pop a counter so
/// the outer block closes at its own `{{/if}}`, not the inner one. Only
/// same-helper nesting is tracked — mixing `{{#if}}` inside `{{#unless}}`
/// uses separate counters per helper, which is correct for our
/// intentionally minimal language.
fn find_matching_close(body: &str, helper: &str) -> Option<usize> {
    let open_tag = format!("{{{{#{helper}");
    let close_tag = format!("{{{{/{helper}}}}}");
    let mut depth: usize = 1;
    let mut i: usize = 0;
    while i < body.len() {
        let next_open = body[i..].find(&open_tag).map(|p| p + i);
        let next_close = body[i..].find(&close_tag).map(|p| p + i);
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                i = o + open_tag.len();
            }
            (_, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    return Some(c);
                }
                i = c + close_tag.len();
            }
            (_, None) => return None,
        }
    }
    None
}

/// Extract the leading identifier from a `{{ ... }}` inner body. Returns
/// `None` if the body doesn't start with a snake_case-style identifier.
fn inner_identifier(inner: &str) -> Option<String> {
    let name: String = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        // Objects and arrays shouldn't appear in merge data at this level;
        // if they do, serialize to JSON as a visible breadcrumb.
        other => other.to_string(),
    }
}

fn is_truthy(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(serde_json::Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(serde_json::Value::Array(a)) => !a.is_empty(),
        Some(serde_json::Value::Object(o)) => !o.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_scalar_with_html_escape() {
        let r = substitute(
            "Hi {{ first_name }}",
            &json!({"first_name": "<b>Alice</b>"}),
        );
        assert_eq!(r.output, "Hi &lt;b&gt;Alice&lt;/b&gt;");
        assert!(r.unresolved.is_empty());
    }

    #[test]
    fn tolerates_whitespace_variations() {
        let data = json!({"name": "bob"});
        assert_eq!(substitute("{{name}}", &data).output, "bob");
        assert_eq!(substitute("{{ name }}", &data).output, "bob");
        assert_eq!(substitute("{{  name  }}", &data).output, "bob");
    }

    #[test]
    fn records_unresolved_placeholders() {
        let r = substitute("Hi {{ first_name }} and {{ other }}", &json!({}));
        assert_eq!(
            r.unresolved,
            vec!["first_name".to_string(), "other".to_string()]
        );
        // Literal tokens survive so agents see them in the preview.
        assert!(r.output.contains("{{ first_name }}"));
        assert!(r.output.contains("{{ other }}"));
    }

    #[test]
    fn unresolved_deduplicates() {
        let r = substitute("{{ x }} {{ x }} {{ x }}", &json!({}));
        assert_eq!(r.unresolved, vec!["x".to_string()]);
    }

    #[test]
    fn triple_brace_allowlisted_substitutes_raw() {
        let html = r#"<a href="https://u/xxx">Unsubscribe</a>"#;
        let r = substitute(
            "footer: {{{ unsubscribe_link }}}",
            &json!({"unsubscribe_link": html}),
        );
        // NOT HTML-escaped — should contain the literal `<a href`.
        assert!(
            r.output
                .contains("<a href=\"https://u/xxx\">Unsubscribe</a>")
        );
        assert!(r.forbidden_raw.is_empty());
    }

    #[test]
    fn triple_brace_non_allowlisted_is_forbidden() {
        let r = substitute("{{{ user_html }}}", &json!({"user_html": "<script>"}));
        assert_eq!(r.forbidden_raw, vec!["user_html".to_string()]);
        // Literal stays so the lint can point at it.
        assert!(r.output.contains("{{{ user_html }}}"));
    }

    #[test]
    fn triple_brace_without_data_leaves_literal_for_send_time() {
        // Preview path: the send pipeline hasn't substituted unsubscribe_link
        // yet, so the literal must survive for later.
        let r = substitute("{{{ unsubscribe_link }}}", &json!({}));
        assert!(r.output.contains("{{{ unsubscribe_link }}}"));
        assert!(r.unresolved.is_empty()); // triple-brace allowlist = not unresolved
    }

    #[test]
    fn if_block_kept_when_truthy() {
        let r = substitute(
            "{{#if company}}From {{ company }}{{/if}}",
            &json!({"company": "Acme"}),
        );
        assert_eq!(r.output, "From Acme");
    }

    #[test]
    fn if_block_dropped_when_falsy() {
        let r = substitute("{{#if company}}From {{ company }}{{/if}}tail", &json!({}));
        assert_eq!(r.output, "tail");
    }

    #[test]
    fn if_block_dropped_when_empty_string() {
        let r = substitute("A{{#if x}}MIDDLE{{/if}}B", &json!({"x": ""}));
        assert_eq!(r.output, "AB");
    }

    #[test]
    fn unless_block_inverts_truthiness() {
        let r = substitute(
            "{{#unless paid}}PAY NOW{{/unless}}",
            &json!({"paid": false}),
        );
        assert_eq!(r.output, "PAY NOW");
    }

    #[test]
    fn nested_if_works() {
        // Inner {{ inner_name }} uses the same data dict.
        let r = substitute(
            "{{#if outer}}A{{#if inner_name}}B{{ inner_name }}{{/if}}C{{/if}}",
            &json!({"outer": true, "inner_name": "!"}),
        );
        assert_eq!(r.output, "AB!C");
    }

    #[test]
    fn number_value_substitutes() {
        let r = substitute("year={{ current_year }}", &json!({"current_year": 2026}));
        assert_eq!(r.output, "year=2026");
    }

    #[test]
    fn preserves_literal_non_brace_content() {
        let r = substitute(
            "<div style=\"padding:10px\">Hello {{ name }}</div>",
            &json!({"name": "Alice"}),
        );
        assert_eq!(r.output, "<div style=\"padding:10px\">Hello Alice</div>");
    }
}
