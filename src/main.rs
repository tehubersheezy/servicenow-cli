use clap::Parser;
use sn::cli::{AuthSub, Cli, Command, TableSub};
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
    let Cli { global, command } = cli;
    match command {
        Command::Init(args) => sn::cli::init::run(args),
        Command::Auth { sub } => match sub {
            AuthSub::Test => sn::cli::auth::test(&global),
        },
        Command::Profile { sub } => sn::cli::profile::run(sub),
        Command::Introspect => sn::cli::introspect::run(),
        Command::Table { sub } => match sub {
            TableSub::List(args) => sn::cli::table::list(&global, args),
            TableSub::Create(args) => sn::cli::table::create(&global, args),
            TableSub::Get(_) | TableSub::Update(_) | TableSub::Replace(_) | TableSub::Delete(_) => {
                Err(Error::Usage("table subcommand not yet wired".into()))
            }
        },
        _ => Err(Error::Usage("command not implemented yet".into())),
    }
}
