mod cli;
mod config;
mod lsp;
mod matcher;
mod runner;

use anyhow::{Context, Result};
use std::path::Path;

use cli::{Cli, CliCommand};
use config::DprintxConfig;
use matcher::ProfileMatcher;
use runner::DprintRunner;

fn main() -> Result<()> {
    // Prevent infinite recursion when symlinked as `dprint` with no config.
    if std::env::var("DPRINTX_ACTIVE").is_ok() {
        anyhow::bail!(
            "dprintx: recursive call detected — \
             create ~/.config/dprint/dprintx.jsonc or ensure the real dprint is in PATH"
        );
    }

    let cli = Cli::parse();
    let config = load_config(cli.config.as_deref())?;

    // No config — passthrough everything to dprint.
    let Some(config) = config else {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let status = std::process::Command::new("dprint")
            .env("DPRINTX_ACTIVE", "1")
            .args(&args)
            .status()
            .context("cannot run dprint (no dprintx config, falling back to dprint in PATH)")?;
        std::process::exit(status.code().unwrap_or(1));
    };

    // For passthrough commands, we don't need matcher.
    if let CliCommand::Passthrough { ref args } = cli.command {
        let runner = DprintRunner::new(&config);
        runner.passthrough_raw(args)?;
        return Ok(());
    }

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

fn load_config(config_path: Option<&str>) -> Result<Option<DprintxConfig>> {
    match config_path {
        // Explicit --config path: must exist and be valid.
        Some(path) => DprintxConfig::load(Path::new(path)).map(Some),
        // Default path: None if file doesn't exist, error if invalid.
        None => DprintxConfig::try_load_default(),
    }
}

/// Show which config would be used for a given file.
fn cmd_config(matcher: &ProfileMatcher, config: &DprintxConfig, file: Option<&str>) -> Result<()> {
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
