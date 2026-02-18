# dprint-mconf

A wrapper around [dprint](https://dprint.dev/) that adds per-file config profiles.

## Why?

dprint doesn't support per-file config overrides ([#996](https://github.com/dprint/dprint/issues/996)). This wrapper
selects the right dprint config based on file path using glob rules.

Also adds unified diff output for `dprint check` ([#1092](https://github.com/dprint/dprint/issues/1092)) with optional
pager support.

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
  "diff_pager": "delta -s",
  "lsp_rewrite_uris": true
}
```

Rules in `match` are evaluated top-to-bottom, first match wins. Files not matching any rule are skipped. Use
`"**": "profile"` as a catch-all.

### diff_pager

When `diff_pager` is set, `dprint check` produces unified diff output instead of dprint's default format:

- **stdout is TTY** → pipes through the pager (e.g. `delta -s`)
- **stdout is pipe/redirect** → raw unified diff

```bash
dprint-mconf check              # pretty diff via delta
dprint-mconf check > fix.patch  # unified diff to file
```

Without `diff_pager`, `dprint check` behaves exactly like the original dprint.

### Local config overrides

Projects can define local formatting rules that override the matched profile.

**How it works:**

1. For each file being formatted, dprint-mconf walks up the directory tree looking for `dprint.json` or `dprint.jsonc`
   (stops at the first one found)
2. If found, it reads the local config and injects the matched profile path into `extends`
3. A temporary merged config is written and passed to dprint instead of the profile config
4. The temp file is auto-deleted when the command finishes (RAII guard)

Since dprint applies `extends` first and then overlays local settings on top, the local config takes precedence.

**Example:**

```jsonc
// ~/projects/my-app/dprint.json — only the overrides you care about
{
  "yaml": {
    "commentSpacing": "ignore"
  }
}
```

When formatting files under `~/projects/my-app/`, dprint-mconf generates a temporary config equivalent to:

```jsonc
{
  "extends": "/home/user/.config/dprint/dprint-default.jsonc",
  "yaml": {
    "commentSpacing": "ignore"
  }
}
```

**`extends` handling:**

| Local config `extends`       | Result                                          |
| ---------------------------- | ----------------------------------------------- |
| absent                       | set to profile path                             |
| `"https://example.com/base"` | `["/profile/path", "https://example.com/base"]` |
| `["a.json", "b.json"]`       | `["/profile/path", "a.json", "b.json"]`         |

The profile path is always prepended so that local settings win.

**Temp file location:** `$XDG_RUNTIME_DIR/dprint-mconf/` (per-user, mode 700). Falls back to `$TMPDIR/dprint-mconf/` if
`XDG_RUNTIME_DIR` is unavailable. Files are named `merged-{pid}-{seq}.json` and cleaned up automatically.

If no local config is found, the profile config is used directly — no temp file is created.

### LSP URI rewriting

dprint matches files by extension, so extensionless files (e.g. shell scripts named `myscript`, Lua scripts without
`.lua`) are silently skipped during LSP formatting.

When `lsp_rewrite_uris` is enabled, the proxy tracks `languageId` from `textDocument/didOpen` and rewrites URIs
forwarded to the dprint backend by appending the correct extension (e.g. `file:///path/myscript` →
`file:///path/myscript.sh` for `languageId=sh`). If the file already has the correct extension, no rewrite happens.

```jsonc
{
  "lsp_rewrite_uris": true
}
```

Default: `false` (transparent passthrough).

Supported languages:

| languageId      | Extension   |
| --------------- | ----------- |
| go              | .go         |
| lua             | .lua        |
| json            | .json       |
| jsonc           | .jsonc      |
| yaml            | .yaml       |
| markdown        | .md         |
| python          | .py         |
| rust            | .rs         |
| typescript      | .ts         |
| typescriptreact | .tsx        |
| javascript      | .js         |
| javascriptreact | .jsx        |
| sh / bash / zsh | .sh         |
| toml            | .toml       |
| css             | .css        |
| html            | .html       |
| sql             | .sql        |
| dockerfile      | .Dockerfile |
| graphql         | .graphql    |

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
cargo install --git https://github.com/mocksoul/dprint-mconf
```

### Transparent dprint replacement

Symlink `dprint-mconf` as `dprint` earlier in your `PATH` — it becomes a fully transparent drop-in replacement. All
unknown commands and flags are forwarded to the real dprint binary (configured via `"dprint"` in `mconf.jsonc`):

```bash
ln -sf ~/.cargo/bin/dprint-mconf ~/.local/bin/dprint
```

Now `dprint fmt`, `dprint check`, `dprint lsp` etc. all go through mconf automatically. No changes needed in editor
configs, CI scripts, or muscle memory.
