//! The entry point of the `soukoban` command line tool.

use std::process::ExitCode;

mod app;
mod cli;
mod commands;
mod error;
mod input;
mod ui;

fn main() -> ExitCode {
    app::run(cli::Cli::parse())
}
