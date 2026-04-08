use crate::cli::{
    ReportAction, ReportDeliverabilityArgs, ReportEngagementArgs, ReportLinksArgs, ReportShowArgs,
};
use crate::error::AppError;
use crate::output::Format;

/// Stub for Phase 6 Tasks 7-8. Returns a not_implemented error so the CLI
/// surface compiles. The DB aggregation helpers (`Db::report_summary`,
/// `report_links`, `report_deliverability`) and full implementations of these
/// command handlers are the next thing to land — see
/// `docs/plans/2026-04-08-phase-6-webhooks-reports.md` Tasks 7 and 8.
pub fn run(_format: Format, action: ReportAction) -> Result<(), AppError> {
    let cmd = match action {
        ReportAction::Show(_) => "report show",
        ReportAction::Links(_) => "report links",
        ReportAction::Engagement(_) => "report engagement",
        ReportAction::Deliverability(_) => "report deliverability",
    };
    Err(AppError::BadInput {
        code: "report_not_implemented".into(),
        message: format!("`{cmd}` is not yet implemented in v0.1.1"),
        suggestion: "Phase 6 Tasks 7-8 will land in v0.1.2 (see docs/plans/2026-04-08-phase-6-webhooks-reports.md)".into(),
    })
}

#[allow(dead_code)]
fn _suppress_unused(
    args: ReportShowArgs,
    _l: ReportLinksArgs,
    _e: ReportEngagementArgs,
    _d: ReportDeliverabilityArgs,
) {
    let _ = args.broadcast_id;
}
