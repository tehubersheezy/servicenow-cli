//! `sn cicd` — the CI/CD API (`/api/sn_cicd/...`): app repository, update-set
//! lifecycle, ATF runs, and the progress records their async jobs report.

use crate::cli::{AppSub, AtfSub, GlobalFlags, ProgressArgs, UpdateSetSub};
use crate::error::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum CicdSub {
    /// App repository operations (install, publish, rollback).
    App {
        #[command(subcommand)]
        sub: AppSub,
    },
    /// Update Set lifecycle operations.
    #[command(name = "updateset")]
    UpdateSet {
        #[command(subcommand)]
        sub: UpdateSetSub,
    },
    /// Automated Test Framework operations.
    Atf {
        #[command(subcommand)]
        sub: AtfSub,
    },
    /// Get pipeline/deployment progress by ID.
    Progress(ProgressArgs),
}

pub fn run(global: &GlobalFlags, sub: CicdSub) -> Result<()> {
    use crate::cli::{app, atf, progress, update_set};
    match sub {
        CicdSub::App { sub } => match sub {
            AppSub::Install(args) => app::install(global, args),
            AppSub::Publish(args) => app::publish(global, args),
            AppSub::Rollback(args) => app::rollback(global, args),
        },
        CicdSub::UpdateSet { sub } => match sub {
            UpdateSetSub::Create(args) => update_set::create(global, args),
            UpdateSetSub::Retrieve(args) => update_set::retrieve(global, args),
            UpdateSetSub::Preview(args) => update_set::preview(global, args),
            UpdateSetSub::Commit(args) => update_set::commit(global, args),
            UpdateSetSub::CommitMultiple(args) => update_set::commit_multiple(global, args),
            UpdateSetSub::BackOut(args) => update_set::back_out(global, args),
        },
        CicdSub::Atf { sub } => match sub {
            AtfSub::Run(args) => atf::run(global, args),
            AtfSub::Results(args) => atf::results(global, args),
        },
        CicdSub::Progress(args) => progress::run(global, args),
    }
}
