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
