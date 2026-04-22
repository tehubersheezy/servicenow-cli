use crate::cli::Cli;
use crate::error::{Error, Result};
use crate::output::{emit_value, Format};
use clap::{Arg, Command as ClapCommand, CommandFactory};
use serde_json::{json, Value};
use std::io;

pub fn run() -> Result<()> {
    let cmd = Cli::command();
    let tree = describe_command(&cmd, "sn");
    emit_value(io::stdout().lock(), &tree, Format::Auto.resolve())
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn describe_command(cmd: &ClapCommand, name: &str) -> Value {
    let args: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| !a.is_hide_set())
        .map(describe_arg)
        .collect();
    let subs: Vec<Value> = cmd
        .get_subcommands()
        .map(|sc| describe_command(sc, sc.get_name()))
        .collect();
    json!({
        "name": name,
        "about": cmd.get_about().map(|s| s.to_string()),
        "args": args,
        "subcommands": subs,
    })
}

fn describe_arg(a: &Arg) -> Value {
    let aliases: Vec<&str> = a.get_all_aliases().unwrap_or_default();
    let possible_values: Vec<String> = a
        .get_possible_values()
        .iter()
        .map(|p| p.get_name().to_string())
        .collect();
    json!({
        "name": a.get_id().as_str(),
        "long": a.get_long(),
        "short": a.get_short(),
        "aliases": aliases,
        "help": a.get_help().map(|s| s.to_string()),
        "required": a.is_required_set(),
        "takes_value": !a.get_num_args().is_some_and(|n| n.min_values() == 0),
        "possible_values": possible_values,
    })
}
