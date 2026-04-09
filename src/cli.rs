use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "mailing-list-cli",
    version,
    about = "Newsletter and mailing list management from your terminal. Built for AI agents on top of email-cli.",
    long_about = None,
)]
pub struct Cli {
    /// Force JSON output even on a TTY
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print the JSON capability manifest
    AgentInfo,
    /// Run a system health check
    Health,
    /// Self-update from GitHub Releases
    Update {
        #[arg(long)]
        check: bool,
    },
    /// Manage the skill file installed in agent platforms
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Manage mailing lists (audiences)
    List {
        #[command(subcommand)]
        action: ListAction,
    },
    /// Manage contacts within lists
    Contact {
        #[command(subcommand)]
        action: ContactAction,
    },
    /// Manage tags (n:m with contacts)
    Tag {
        #[command(subcommand)]
        action: TagAction,
    },
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
    /// Manage HTML email templates (plain HTML + `{{ var }}` / `{{#if}}` substitution, 6 lint rules)
    Template {
        #[command(subcommand)]
        action: TemplateAction,
    },
    /// Manage named, targeted broadcasts (campaigns)
    Broadcast {
        #[command(subcommand)]
        action: BroadcastAction,
    },
    /// Webhook event ingestion via polling
    Webhook {
        #[command(subcommand)]
        action: WebhookAction,
    },
    /// Shorthand for `webhook poll`
    Event {
        #[command(subcommand)]
        action: EventAction,
    },
    /// Analytics reports (per-broadcast, per-link, engagement, deliverability)
    Report {
        #[command(subcommand)]
        action: ReportAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillAction {
    /// Install skill files into Claude / Codex / Gemini paths
    Install,
    /// Show installed-skill status
    Status,
}

#[derive(Subcommand, Debug)]
pub enum ListAction {
    /// Create a new list (also creates a Resend audience via email-cli)
    Create(ListCreateArgs),
    /// List all lists with subscriber counts
    #[command(visible_alias = "ls")]
    List,
    /// Show one list's details
    Show(ListShowArgs),
}

#[derive(Args, Debug)]
pub struct ListCreateArgs {
    /// The list name (used as the Resend audience name too)
    pub name: String,
    /// Optional human-readable description
    #[arg(long)]
    pub description: Option<String>,
}

#[derive(Args, Debug)]
pub struct ListShowArgs {
    /// The list id
    pub id: i64,
}

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
    /// Set a custom field value on a contact
    Set(ContactSetArgs),
    /// Show a contact's full details
    Show(ContactShowArgs),
    /// Bulk-import contacts from a CSV file
    Import(ContactImportArgs),
    /// GDPR Article 17 erasure: atomically delete a contact + all owned
    /// child rows and insert a `gdpr_erasure` suppression tombstone so
    /// the address cannot be re-added without manual intervention.
    /// Requires `--confirm` because the operation is irreversible.
    Erase(ContactEraseArgs),
}

#[derive(Args, Debug)]
pub struct ContactEraseArgs {
    /// Email address of the contact to erase
    pub email: String,
    /// Required confirmation flag — erase is irreversible
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Args, Debug)]
pub struct ContactTagArgs {
    /// Contact email
    pub email: String,
    /// Tag name
    pub tag: String,
}

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

#[derive(Args, Debug)]
pub struct ContactListArgs {
    /// Restrict to a list id (omit to search across all lists)
    #[arg(long)]
    pub list: Option<i64>,
    /// JSON filter (SegmentExpr). Use `--filter-json '{"kind":"atom",...}'` or
    /// pair with `--filter-json-file <path>` to read from disk for readability.
    #[arg(long = "filter-json")]
    pub filter_json: Option<String>,
    /// Read JSON filter from a file (alternative to --filter-json for long expressions)
    #[arg(long = "filter-json-file")]
    pub filter_json_file: Option<std::path::PathBuf>,
    /// Maximum number of contacts to return (max 10000)
    #[arg(long, default_value = "100")]
    pub limit: usize,
    /// Cursor (last contact id seen); start from the beginning if omitted
    #[arg(long)]
    pub cursor: Option<i64>,
}

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

#[derive(Args, Debug)]
pub struct ContactSetArgs {
    /// Contact email
    pub email: String,
    /// Field key
    pub field: String,
    /// Field value (coerced to the field's declared type)
    pub value: String,
}

#[derive(Args, Debug)]
pub struct ContactShowArgs {
    /// Contact email
    pub email: String,
}

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
    /// Filter as a JSON SegmentExpr. Use `--filter-json '{"kind":"atom",...}'`
    /// or pair with `--filter-json-file <path>` for multi-line readability.
    #[arg(long = "filter-json")]
    pub filter_json: Option<String>,
    /// Read JSON filter from a file (alternative to --filter-json)
    #[arg(long = "filter-json-file")]
    pub filter_json_file: Option<std::path::PathBuf>,
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

#[derive(Subcommand, Debug)]
pub enum TemplateAction {
    /// Create a new template from an HTML file (or a built-in scaffold if no file)
    Create(TemplateCreateArgs),
    /// List all templates
    #[command(visible_alias = "ls")]
    List,
    /// Print a template's HTML source
    Show(TemplateShowArgs),
    /// Render a template with merge data (returns JSON envelope with embedded html/text)
    Render(TemplateRenderArgs),
    /// Write a rendered preview to disk for iteration (and optionally open in browser)
    Preview(TemplatePreviewArgs),
    /// Run the lint rule set against a template
    Lint(TemplateLintArgs),
    /// Delete a template
    Rm(TemplateRmArgs),
}

#[derive(Args, Debug)]
pub struct TemplateCreateArgs {
    /// Template name (snake_case)
    pub name: String,
    /// Subject line (required). May itself contain `{{ var }}` merge tags
    /// (e.g. `--subject "Welcome, {{ first_name }}"`), which are resolved at
    /// send time against the contact's merge data.
    #[arg(long)]
    pub subject: Option<String>,
    /// Import HTML body from this file path. If omitted, a built-in scaffold is used.
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
    /// Leave `{{{ unsubscribe_link }}}` and `{{{ physical_address_footer }}}` literal
    /// instead of substituting placeholder stubs. By default, both are auto-injected
    /// with realistic stub values so the output is viewable HTML (matching `template
    /// preview`). Use `--raw` when piping to a downstream substituter or custom sender.
    #[arg(long)]
    pub raw: bool,
}

#[derive(Args, Debug)]
pub struct TemplatePreviewArgs {
    pub name: String,
    /// JSON file with merge data (an object: { "first_name": "Alice" })
    #[arg(long = "with-data")]
    pub with_data: Option<std::path::PathBuf>,
    /// Output directory for preview artifacts. Defaults to $MLC_CACHE_DIR/preview/<name>/.
    /// Three files are written: `index.html` (rendered HTML), `plain.txt` (text fallback),
    /// and `subject.txt` (rendered subject).
    #[arg(long = "out-dir")]
    pub out_dir: Option<std::path::PathBuf>,
    /// Open the rendered HTML in the default browser (macOS `open`, Linux `xdg-open`, Windows `start`)
    #[arg(long)]
    pub open: bool,
}

#[derive(Args, Debug)]
pub struct TemplateLintArgs {
    pub name: String,
}

#[derive(Args, Debug)]
pub struct TemplateRmArgs {
    pub name: String,
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Subcommand, Debug)]
pub enum BroadcastAction {
    /// Stage a new broadcast in draft status
    Create(BroadcastCreateArgs),
    /// Send a single test copy via email-cli send
    Preview(BroadcastPreviewArgs),
    /// Move a draft broadcast into scheduled status
    Schedule(BroadcastScheduleArgs),
    /// Send the broadcast now (runs the full pipeline). Safe to re-run on
    /// an interrupted broadcast — already-sent recipients are skipped.
    Send(BroadcastSendArgs),
    /// Resume an interrupted broadcast send. Identical behavior to `send`
    /// (both skip already-sent recipients), but the name makes intent
    /// explicit when recovering from a crash or kill mid-send.
    Resume(BroadcastSendArgs),
    /// Cancel a draft or scheduled broadcast
    Cancel(BroadcastCancelArgs),
    /// List recent broadcasts
    #[command(visible_alias = "ls")]
    List(BroadcastListArgs),
    /// Show full details for a broadcast
    Show(BroadcastShowArgs),
}

#[derive(Args, Debug)]
pub struct BroadcastCreateArgs {
    /// Broadcast name (agents use this as a memorable identifier)
    #[arg(long)]
    pub name: String,
    /// Template name to send
    #[arg(long)]
    pub template: String,
    /// Target: `list:<name>` or `segment:<name>`
    #[arg(long)]
    pub to: String,
}

#[derive(Args, Debug)]
pub struct BroadcastPreviewArgs {
    pub id: i64,
    #[arg(long)]
    pub to: String,
}

#[derive(Args, Debug)]
pub struct BroadcastScheduleArgs {
    pub id: i64,
    /// RFC 3339 timestamp (e.g. 2026-04-09T12:00:00Z)
    #[arg(long)]
    pub at: String,
}

#[derive(Args, Debug)]
pub struct BroadcastSendArgs {
    pub id: i64,
    /// Force-acquire the send lock even if another process appears to hold
    /// it. USE WITH CAUTION: only after confirming the other process is
    /// truly dead (e.g., `ps aux | grep mailing-list-cli`). Risk: double-send
    /// if the other process is still alive and rendering chunks.
    #[arg(long)]
    pub force_unlock: bool,
    /// Dry-run: resolve recipients, run preflight checks, render every
    /// chunk, and report what WOULD be sent — but do not call email-cli
    /// or modify any broadcast state. Exit 0 with the projected counts.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct BroadcastCancelArgs {
    pub id: i64,
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Args, Debug)]
pub struct BroadcastListArgs {
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long, default_value = "50")]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct BroadcastShowArgs {
    pub id: i64,
}

#[derive(Subcommand, Debug)]
pub enum WebhookAction {
    /// Poll email-cli for delivery status updates (alias of `event poll`)
    Poll(WebhookPollArgs),
}

#[derive(Args, Debug)]
pub struct WebhookPollArgs {
    #[arg(long)]
    pub reset: bool,
}

#[derive(Subcommand, Debug)]
pub enum EventAction {
    /// Poll email-cli for events
    Poll(WebhookPollArgs),
}

#[derive(Subcommand, Debug)]
pub enum ReportAction {
    /// Show per-broadcast summary stats
    Show(ReportShowArgs),
    /// Show per-link click counts for a broadcast
    Links(ReportLinksArgs),
    /// Show engagement across a list/segment
    Engagement(ReportEngagementArgs),
    /// Show rolling-window bounce rate / complaint rate
    Deliverability(ReportDeliverabilityArgs),
}

#[derive(Args, Debug)]
pub struct ReportShowArgs {
    pub broadcast_id: i64,
}

#[derive(Args, Debug)]
pub struct ReportLinksArgs {
    pub broadcast_id: i64,
}

#[derive(Args, Debug)]
pub struct ReportEngagementArgs {
    #[arg(long)]
    pub list: Option<String>,
    #[arg(long)]
    pub segment: Option<String>,
    #[arg(long, default_value = "30")]
    pub days: i64,
}

#[derive(Args, Debug)]
pub struct ReportDeliverabilityArgs {
    #[arg(long, default_value = "7")]
    pub days: i64,
}
