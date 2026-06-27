//! Hayate CLI application.

mod cli;
mod output;
mod subcmd;
mod words;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::Cli;
use compio::runtime::spawn;

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

    // Shared cancellation flag for graceful shutdown on Ctrl+C.
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);

    // compio thread-per-core runtime.
    let runtime = compio::runtime::Runtime::new()?;
    runtime.block_on(async {
        // Spawn a signal handler that sets the cancellation flag.
        spawn(async move {
            // On Unix, compio::signal::ctrl_c() works. On Windows, it wraps
            // SetConsoleCtrlHandler. Both should be reliable with compio.
            let _ = compio::signal::ctrl_c().await;
            cancelled_clone.store(true, Ordering::SeqCst);
            // Small grace period for logs to flush, then force exit.
            compio::time::sleep(std::time::Duration::from_millis(1500)).await;
            std::process::exit(130);
        })
        .detach();

        let res = subcmd::dispatch(cli, cancelled).await;
        if let Err(err) = res {
            output::print_error(&err);
            std::process::exit(1);
        }
        Ok(())
    })
}
