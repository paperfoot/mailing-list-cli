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
