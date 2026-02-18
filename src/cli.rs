use clap::{Parser, Subcommand};

/// dprint-mconf: dprint wrapper with per-file config profiles.
///
/// Selects dprint config based on file path using glob rules
/// defined in ~/.config/dprint/mconf.jsonc.
#[derive(Parser, Debug)]
#[command(name = "dprint", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Override mconf config path.
    #[arg(long, global = true)]
    pub mconf: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Format files.
    Fmt {
        /// Read from stdin and write to stdout. Requires a filename for config matching.
        #[arg(long)]
        stdin: Option<String>,

        /// Files to format.
        #[arg(trailing_var_arg = true)]
        files: Vec<String>,
    },

    /// Check if files are formatted.
    Check {
        /// Files to check.
        #[arg(trailing_var_arg = true)]
        files: Vec<String>,
    },

    /// Show resolved config for a file.
    Config {
        /// File path to resolve config for.
        file: Option<String>,
    },

    /// Clear dprint cache.
    #[command(name = "clear-cache")]
    ClearCache,

    /// List files that would be formatted.
    #[command(name = "output-file-paths")]
    OutputFilePaths,

    /// Start LSP server (profile-aware proxy).
    Lsp,
}
