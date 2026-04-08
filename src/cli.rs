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
