/// Parsed CLI result.
#[derive(Debug)]
pub struct Cli {
    /// Override config path.
    pub config: Option<String>,
    /// Parsed command.
    pub command: CliCommand,
}

#[derive(Debug)]
pub enum CliCommand {
    /// Format files.
    Fmt {
        stdin: Option<String>,
        files: Vec<String>,
    },
    /// Check if files are formatted.
    Check { files: Vec<String> },
    /// Show resolved config for a file.
    Config { file: Option<String> },
    /// List files that would be formatted.
    OutputFilePaths,
    /// Start LSP server.
    Lsp,
    /// Passthrough to real dprint (unknown command or --help etc).
    Passthrough { args: Vec<String> },
}

impl Cli {
    /// Parse CLI from env args.
    /// Known commands are parsed by us; everything else is passthrough.
    pub fn parse() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        Self::parse_from(&args)
    }

    fn parse_from(args: &[String]) -> Self {
        let mut config: Option<String> = None;
        let mut rest: Vec<String> = Vec::new();

        // Extract --config <path> from anywhere in args.
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--config" {
                if i + 1 < args.len() {
                    config = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            } else if let Some(val) = args[i].strip_prefix("--config=") {
                config = Some(val.to_string());
                i += 1;
                continue;
            }
            rest.push(args[i].clone());
            i += 1;
        }

        // No subcommand or help flags → passthrough.
        if rest.is_empty() {
            return Self {
                config,
                command: CliCommand::Passthrough { args: rest },
            };
        }

        let subcmd = rest[0].as_str();
        let sub_args = &rest[1..];

        let command = match subcmd {
            "fmt" => Self::parse_fmt(sub_args),
            "check" => Self::parse_check(sub_args),
            "config" => CliCommand::Config {
                file: sub_args.first().cloned(),
            },
            "output-file-paths" => CliCommand::OutputFilePaths,
            "lsp" => CliCommand::Lsp,
            // Everything else: --help, -h, --version, -V, license, completions, etc.
            _ => CliCommand::Passthrough { args: rest },
        };

        Self { config, command }
    }

    fn parse_fmt(args: &[String]) -> CliCommand {
        let mut stdin: Option<String> = None;
        let mut files: Vec<String> = Vec::new();

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--stdin" => {
                    if i + 1 < args.len() {
                        stdin = Some(args[i + 1].clone());
                        i += 2;
                        continue;
                    }
                }
                // Pass through help to real dprint.
                "-h" | "--help" => {
                    let mut passthrough = vec!["fmt".to_string()];
                    passthrough.extend_from_slice(args);
                    return CliCommand::Passthrough { args: passthrough };
                }
                other => files.push(other.to_string()),
            }
            i += 1;
        }

        CliCommand::Fmt { stdin, files }
    }

    fn parse_check(args: &[String]) -> CliCommand {
        let mut files: Vec<String> = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-h" | "--help" => {
                    let mut passthrough = vec!["check".to_string()];
                    passthrough.extend_from_slice(args);
                    return CliCommand::Passthrough { args: passthrough };
                }
                other => files.push(other.to_string()),
            }
        }

        CliCommand::Check { files }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn test_fmt_stdin() {
        let cli = Cli::parse_from(&args("fmt --stdin test.yaml"));
        assert!(matches!(
            cli.command,
            CliCommand::Fmt { stdin: Some(_), .. }
        ));
    }

    #[test]
    fn test_fmt_files() {
        let cli = Cli::parse_from(&args("fmt a.go b.go"));
        if let CliCommand::Fmt { stdin, files } = &cli.command {
            assert!(stdin.is_none());
            assert_eq!(files, &["a.go", "b.go"]);
        } else {
            panic!("expected Fmt");
        }
    }

    #[test]
    fn test_check_files() {
        let cli = Cli::parse_from(&args("check a.yaml"));
        if let CliCommand::Check { files } = &cli.command {
            assert_eq!(files, &["a.yaml"]);
        } else {
            panic!("expected Check");
        }
    }

    #[test]
    fn test_unknown_passthrough() {
        let cli = Cli::parse_from(&args("license"));
        assert!(matches!(cli.command, CliCommand::Passthrough { .. }));
    }

    #[test]
    fn test_help_passthrough() {
        let cli = Cli::parse_from(&args("--help"));
        assert!(matches!(cli.command, CliCommand::Passthrough { .. }));
    }

    #[test]
    fn test_version_passthrough() {
        let cli = Cli::parse_from(&args("-V"));
        assert!(matches!(cli.command, CliCommand::Passthrough { .. }));
    }

    #[test]
    fn test_config_extracted() {
        let cli = Cli::parse_from(&args("--config /tmp/test.jsonc fmt a.go"));
        assert_eq!(cli.config.as_deref(), Some("/tmp/test.jsonc"));
        assert!(matches!(cli.command, CliCommand::Fmt { .. }));
    }

    #[test]
    fn test_config_equals() {
        let cli = Cli::parse_from(&args("--config=/tmp/test.jsonc check"));
        assert_eq!(cli.config.as_deref(), Some("/tmp/test.jsonc"));
        assert!(matches!(cli.command, CliCommand::Check { .. }));
    }

    #[test]
    fn test_no_args_passthrough() {
        let cli = Cli::parse_from(&args(""));
        assert!(matches!(cli.command, CliCommand::Passthrough { .. }));
    }

    #[test]
    fn test_fmt_help_passthrough() {
        let cli = Cli::parse_from(&args("fmt --help"));
        assert!(matches!(cli.command, CliCommand::Passthrough { .. }));
    }
}
