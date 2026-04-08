mod cli;
mod commands;
mod config;
mod db;
mod email_cli;
mod error;
mod models;
mod output;
mod paths;
mod segment;

use clap::Parser;
use cli::{Cli, Command};
use output::Format;
use std::process::ExitCode;

fn main() -> ExitCode {
    let parsed = Cli::parse();
    let format = Format::detect(parsed.json);

    let result = match parsed.command {
        Command::AgentInfo => {
            commands::agent_info::run();
            Ok(())
        }
        Command::Health => commands::health::run(format),
        Command::Update { check } => commands::update::run(format, check),
        Command::Skill { action } => commands::skill::run(format, action),
        Command::List { action } => commands::list::run(format, action),
        Command::Contact { action } => commands::contact::run(format, action),
    };

    match result {
        Ok(()) => ExitCode::from(error::ExitCode::Success.as_i32() as u8),
        Err(err) => {
            output::error(format, &err);
            ExitCode::from(err.exit_code().as_i32() as u8)
        }
    }
}
