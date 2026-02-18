mod cli;
mod config;
mod matcher;

use anyhow::Result;
use clap::Parser;

use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // TODO: implement commands
    eprintln!("dprint-mconf: not yet implemented");
    eprintln!("parsed args: {cli:?}");

    Ok(())
}
