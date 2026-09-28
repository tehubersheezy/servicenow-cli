//! `sn flow` — Flow Designer flows and subflows.
//!
//! The group's verbs live in submodules: [`debug`] holds the execution
//! debugging verbs (`runs`, `debug`, `steps`, `logs`, `why-not`, `tail`), which
//! read the documented flow-engine tables and are flattened in here so they sit
//! directly under `sn flow`.

use crate::cli::GlobalFlags;
use crate::error::Result;
use clap::Subcommand;

pub mod debug;

#[derive(Subcommand, Debug)]
pub enum FlowSub {
    #[command(flatten)]
    Debug(debug::FlowDebugSub),
}

pub fn run(global: &GlobalFlags, sub: FlowSub) -> Result<()> {
    match sub {
        FlowSub::Debug(sub) => debug::run(global, sub),
    }
}
