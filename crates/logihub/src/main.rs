mod cli;
mod gui;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> ExitCode {
    let args = Cli::parse();
    let result = match args.command {
        None | Some(Command::Gui) => gui::run_gui(),
        Some(cmd) => cli::run_command(cmd),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
