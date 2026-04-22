use clap::Parser;
use sn::cli::{Cli, Command};
use sn::error::{Error, Result};
use sn::output::emit_error;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = emit_error(io::stderr().lock(), &err);
            ExitCode::from(err.exit_code() as u8)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => sn::cli::init::run(args),
        Command::Introspect => {
            // Filled in by Task 24.
            println!("{{\"todo\": \"introspect\"}}");
            Ok(())
        }
        _ => Err(Error::Usage("command not implemented yet".into())),
    }
}
