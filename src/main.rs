mod cli;
mod config;
mod lsp;
mod matcher;
mod runner;

use anyhow::{Context, Result};
use std::path::Path;

use cli::{Cli, CliCommand};
use config::MconfConfig;
use matcher::ProfileMatcher;
use runner::DprintRunner;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // For passthrough commands, we don't need mconf config at all.
    if let CliCommand::Passthrough { ref args } = cli.command {
        // Load config just to get the dprint binary path.
        let config = load_config(cli.mconf.as_deref())?;
        let runner = DprintRunner::new(&config);
        runner.passthrough_raw(args)?;
        return Ok(());
    }

    let config = load_config(cli.mconf.as_deref())?;
    let matcher = ProfileMatcher::from_config(&config)?;
    let runner = DprintRunner::new(&config);

    match cli.command {
        CliCommand::Fmt { stdin, files } => {
            if let Some(ref filename) = stdin {
                runner.fmt_stdin(filename, &matcher, &config)?;
            } else if files.is_empty() {
                runner.fmt_all(&matcher, &config)?;
            } else {
                runner.fmt_files(&files, &matcher, &config)?;
            }
        }
        CliCommand::Check { files } => {
            if files.is_empty() {
                runner.check_all(&matcher, &config)?;
            } else {
                runner.check_files(&files, &matcher, &config)?;
            }
        }
        CliCommand::Config { file } => {
            cmd_config(&matcher, &config, file.as_deref())?;
        }
        CliCommand::OutputFilePaths => {
            runner.output_file_paths(&matcher, &config)?;
        }
        CliCommand::Lsp => {
            let proxy = lsp::LspProxy::new(config.dprint_path(), matcher, config);
            proxy.run()?;
        }
        CliCommand::Passthrough { .. } => unreachable!(),
    }

    Ok(())
}

fn load_config(mconf_path: Option<&str>) -> Result<MconfConfig> {
    match mconf_path {
        Some(path) => MconfConfig::load(Path::new(path)),
        None => MconfConfig::load_default(),
    }
}

/// Show which config would be used for a given file.
fn cmd_config(matcher: &ProfileMatcher, config: &MconfConfig, file: Option<&str>) -> Result<()> {
    match file {
        Some(f) => {
            let abs_path = std::fs::canonicalize(f).unwrap_or_else(|_| std::path::PathBuf::from(f));
            let config_path = matcher
                .resolve_config(&abs_path, config)
                .with_context(|| format!("resolving config for {f}"))?;
            match config_path {
                Some(p) => println!("{}", p.display()),
                None => println!("(no matching profile)"),
            }
        }
        None => {
            println!("dprint: {}", config.dprint_path().display());
            println!("profiles:");
            for (name, value) in &config.profiles {
                if let Some(path) = value.as_str() {
                    println!("  {name}: {}", config::expand_tilde(path).display());
                }
            }
            println!("match rules:");
            for (pattern, profile) in config.match_rules_iter() {
                println!("  {pattern} -> {profile}");
            }
        }
    }
    Ok(())
}
