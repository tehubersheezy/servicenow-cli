use crate::cli::Cli;
use crate::error::Result;
use crate::output::{emit_value, Format};
use clap::{Arg, ArgAction, Command as ClapCommand, CommandFactory};
use serde_json::{json, Value};
use std::io;

pub fn run() -> Result<()> {
    // build() finalizes defaults, actions, and value parsers; without it,
    // num_args/actions are unset and every arg looks like it takes a value.
    let mut cmd = Cli::command();
    cmd.build();
    let tree = describe_command(&cmd, "sn");
    emit_value(io::stdout().lock(), &tree, Format::Auto.resolve())
        .map_err(crate::output::map_stdout_err)
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
    let takes_value = matches!(a.get_action(), ArgAction::Set | ArgAction::Append);
    // Flags (SetTrue/Count) carry a synthetic bool parser; suppress its
    // ["true","false"] so agents don't emit `--flag true`.
    let possible_values: Vec<String> = if takes_value {
        a.get_possible_values()
            .iter()
            .map(|p| p.get_name().to_string())
            .collect()
    } else {
        Vec::new()
    };
    let default_values: Vec<String> = a
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();
    json!({
        "name": a.get_id().as_str(),
        "long": a.get_long(),
        "short": a.get_short(),
        "aliases": aliases,
        "help": a.get_help().map(|s| s.to_string()),
        "required": a.is_required_set(),
        "takes_value": takes_value,
        "positional": a.is_positional(),
        "repeatable": matches!(a.get_action(), ArgAction::Append | ArgAction::Count),
        "default_values": default_values,
        "possible_values": possible_values,
    })
}
