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

#![allow(dead_code, unused_imports)]

pub mod ast;
pub mod compiler;
pub mod parser;

pub use ast::{Atom, EngagementAtom, FieldOp, ListPredicate, SegmentExpr, TagPredicate};
pub use compiler::to_sql_where;
pub use parser::{ParseError, parse};
