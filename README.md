# dprint-mconf

A wrapper around [dprint](https://dprint.dev/) that adds per-file config profiles.

## Why?

dprint doesn't support per-file config overrides ([#996](https://github.com/dprint/dprint/issues/996)).
This wrapper selects the right dprint config based on file path using glob rules.

Also adds unified diff output for `dprint check` ([#1092](https://github.com/dprint/dprint/issues/1092)) with optional pager support.

## How it works

Config file: `~/.config/dprint/mconf.jsonc`

```jsonc
{
  "dprint": "~/.cargo/bin/dprint",
  "profiles": {
    "maintainer": "~/.config/dprint/dprint-maintainer.jsonc",
    "default": "~/.config/dprint/dprint-default.jsonc"
  },
  "match": {
    "**/noc/cmdb/**": "maintainer",
    "**/noc/invapi/**": "maintainer",
    "**": "default"
  },
  "diff_pager": "delta -s"
}
```

Rules in `match` are evaluated top-to-bottom, first match wins. Files not matching any rule are skipped. Use `"**": "profile"` as a catch-all.

### diff_pager

When `diff_pager` is set, `dprint check` produces unified diff output instead of dprint's default format:

- **stdout is TTY** → pipes through the pager (e.g. `delta -s`)
- **stdout is pipe/redirect** → raw unified diff

```bash
dprint-mconf check              # pretty diff via delta
dprint-mconf check > fix.patch  # unified diff to file
```

Without `diff_pager`, `dprint check` behaves exactly like the original dprint.

## CLI

```bash
# stdin — single file, filename is used for config matching (input is read from stdin)
dprint-mconf fmt --stdin path/to/file.yaml < input.yaml

# fmt/check — groups files by profile, calls dprint per group
dprint-mconf fmt
dprint-mconf check
dprint-mconf fmt file1.go file2.yaml   # explicit file list

# list all files that would be formatted (merged from all profiles)
dprint-mconf output-file-paths

# show which config is used
dprint-mconf config              # all profiles and rules
dprint-mconf config path/to/file # resolved config for a file

# LSP proxy — spawns dprint lsp per profile, routes by file URI
dprint-mconf lsp
```

`dprint-mconf check` exits with code 1 if any files need formatting.

Use `--mconf <PATH>` to override the config location (default: `~/.config/dprint/mconf.jsonc`):

```bash
dprint-mconf --mconf /path/to/custom.jsonc fmt
```

All unknown commands and flags are passed through to the real dprint (`--help`, `-V`, `license`, `completions`, etc.).

## Install

```bash
cargo install --path .
```

### Transparent dprint replacement

Symlink `dprint-mconf` as `dprint` earlier in your `PATH` — it becomes a fully transparent drop-in replacement. All unknown commands and flags are forwarded to the real dprint binary (configured via `"dprint"` in `mconf.jsonc`):

```bash
ln -sf ~/.cargo/bin/dprint-mconf ~/.local/bin/dprint
```

Now `dprint fmt`, `dprint check`, `dprint lsp` etc. all go through mconf automatically. No changes needed in editor configs, CI scripts, or muscle memory.
