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
}
