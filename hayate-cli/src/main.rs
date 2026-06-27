//! Hayate CLI application.

mod cli;
mod output;
mod subcmd;
mod words;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::Cli;

fn main() -> Result<()> {
    if std::env::var_os("NO_COLOR").is_none() {
        console::set_colors_enabled(true);
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "-V") {
        println!("v{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    if args.iter().any(|arg| arg == "--version") {
        println!(
            "v{} (commit: {})",
            env!("CARGO_PKG_VERSION"),
            env!("GIT_COMMIT_HASH")
        );
        std::process::exit(0);
    }

    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(err) => {
            let kind = err.kind();
            if kind == clap::error::ErrorKind::DisplayHelp
                || kind == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                || kind == clap::error::ErrorKind::MissingSubcommand
            {
                output::print_banner();
            }
            err.exit();
        }
    };

    if cli.command.is_none() {
        output::print_banner();
        Cli::command().print_help()?;
        println!();
        return Ok(());
    }

    // compio thread-per-core runtime: single OS thread, one io_uring /
    // IOCP / kqueue completion queue, no work-stealing scheduler.
    let runtime = compio::runtime::Runtime::new()?;
    runtime.block_on(async {
        compio::runtime::spawn(async {
            if compio::signal::ctrl_c().await.is_ok() {
                std::process::exit(130);
            }
        })
        .detach();

        let res = subcmd::dispatch(cli).await;
        if let Err(err) = res {
            output::print_error(&err);
            std::process::exit(1);
        }
        Ok(())
    })
}
