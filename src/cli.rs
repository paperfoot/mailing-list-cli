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
    /// Add a contact to a list (also writes through to the Resend audience)
    Add(ContactAddArgs),
    /// List contacts in a list
    #[command(visible_alias = "ls")]
    List(ContactListArgs),
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
}

#[derive(Args, Debug)]
pub struct ContactListArgs {
    /// The list id
    #[arg(long)]
    pub list: i64,
    /// Maximum number of contacts to return
    #[arg(long, default_value = "100")]
    pub limit: usize,
}
