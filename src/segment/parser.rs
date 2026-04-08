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

/// Reserved keys that have a dedicated atom rule. If pest falls through to
/// `keyed_atom` with one of these as the key, it means the dedicated rule
/// failed (e.g. bad duration unit) and we should surface that as an error
/// instead of treating it as a custom field.
const RESERVED_KEYS: &[&str] = &[
    "opened_last",
    "clicked_last",
    "sent_last",
    "inactive_for",
    "never_opened",
    "tag",
    "has_tag",
    "no_tag",
    "list",
    "in_list",
    "not_in_list",
    "bounced",
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
    for next in iter {
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
    for next in iter {
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

    if RESERVED_KEYS.contains(&key.as_str()) {
        return Err(ParseError::new(
            format!("'{key}' is a reserved keyword and cannot be used as a custom field"),
            format!(
                "if you meant the {key} atom, check its syntax (e.g. `{key}:30d` requires a valid duration)"
            ),
        ));
    }

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
