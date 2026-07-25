//! `hayate man` hidden subcommand: renders roff man pages from the clap
//! definitions so the manual never drifts from `--help`. The release pipeline
//! captures this output into `man/hayate.1` for archives and `.deb` packages.

use anyhow::Result;
use clap::CommandFactory;

use crate::cli::Cli;

/// Prints the top-level man page followed by one page per subcommand.
pub fn run() -> Result<()> {
    let cmd = Cli::command();
    let mut out: Vec<u8> = Vec::new();
    clap_mangen::Man::new(cmd.clone()).render(&mut out)?;
    for sub in cmd.get_subcommands() {
        clap_mangen::Man::new(sub.clone()).render(&mut out)?;
    }
    use std::io::Write;
    std::io::stdout().write_all(&out)?;
    Ok(())
}
