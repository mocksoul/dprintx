# dprintx

A wrapper around [dprint](https://dprint.dev/) that adds multi-config support and several missing features.

## Features

- **[Per-file config profiles](#how-it-works)** — select dprint config by file path using glob rules
  ([dprint#996](https://github.com/dprint/dprint/issues/996))
- **[Local config overrides](#local-config-overrides)** — project-level `dprint.json` that merges with the matched
  profile via `extends`
- **[Unified diff output](#diff_pager)** — `dprint check` with real unified diff and optional pager
  ([dprint#1092](https://github.com/dprint/dprint/issues/1092))
- **[LSP proxy](#cli)** — spawns per-profile `dprint lsp` backends, routes requests by file URI
- **[LSP URI rewriting](#lsp-uri-rewriting)** — format extensionless files (shell scripts, etc.) by appending the
  correct extension based on editor's `languageId`
- **[Transparent drop-in](#transparent-dprint-replacement)** — symlink as `dprint`, all unknown commands passthrough to
  the real binary

## How it works

Config file: `~/.config/dprint/dprintx.jsonc`

```jsonc
{
  "dprint": "~/.cargo/bin/dprint",
  "profiles": {
    "maintainer": "~/.config/dprint/dprint-maintainer.jsonc",
    "default": "~/.config/dprint/dprint-default.jsonc",
  },
  "match": {
    "**/noc/cmdb/**": "maintainer",
    "**/noc/invapi/**": "maintainer",
    "**": "default",
  },
  "diff_pager": "delta -s",
  "lsp_rewrite_uris": true,
}
```

Rules in `match` are evaluated top-to-bottom, first match wins. Files not matching any rule are skipped. Use
`"**": "profile"` as a catch-all.

### diff_pager

When `diff_pager` is set, `dprint check` produces unified diff output instead of dprint's default format:

- **stdout is TTY** → pipes through the pager (e.g. `delta -s`)
- **stdout is pipe/redirect** → raw unified diff

```bash
dprintx check              # pretty diff via delta
dprintx check > fix.patch  # unified diff to file
```

Without `diff_pager`, `dprint check` behaves exactly like the original dprint.

### Local config overrides

Projects can define local formatting rules that override the matched profile.

**How it works:**

1. For each file being formatted, dprintx walks up the directory tree looking for `dprint.json` or `dprint.jsonc` (stops
   at the first one found)
2. If found, it reads the local config and injects the matched profile path into `extends`
3. A temporary merged config is written and passed to dprint instead of the profile config
4. The temp file is auto-deleted when the command finishes (RAII guard)

Since dprint applies `extends` first and then overlays local settings on top, the local config takes precedence.

**Example:**

```jsonc
// ~/projects/my-app/dprint.json — only the overrides you care about
{
  "yaml": {
    "commentSpacing": "ignore",
  },
}
```

When formatting files under `~/projects/my-app/`, dprintx generates a temporary config equivalent to:

```jsonc
{
  "extends": "/home/user/.config/dprint/dprint-default.jsonc",
  "yaml": {
    "commentSpacing": "ignore",
  },
}
```

**`extends` handling:**

| Local config `extends`       | Result                                          |
| ---------------------------- | ----------------------------------------------- |
| absent                       | set to profile path                             |
| `"https://example.com/base"` | `["/profile/path", "https://example.com/base"]` |
| `["a.json", "b.json"]`       | `["/profile/path", "a.json", "b.json"]`         |

The profile path is always prepended so that local settings win.

**Temp file location:** `$XDG_RUNTIME_DIR/dprintx/` (per-user, mode 700). Falls back to `$TMPDIR/dprintx/` if
`XDG_RUNTIME_DIR` is unavailable. Files are named `merged-{pid}-{seq}.json` and cleaned up automatically.

If no local config is found, the profile config is used directly — no temp file is created.

### LSP URI rewriting (opt-in)

> **Disabled by default** for compatibility. Enable explicitly with `"lsp_rewrite_uris": true`.

dprint matches files by extension, so extensionless files (e.g. shell scripts named `myscript`, Lua scripts without
`.lua`) are silently skipped during LSP formatting.

When `lsp_rewrite_uris` is enabled, the proxy tracks `languageId` from `textDocument/didOpen` and rewrites URIs
forwarded to the dprint backend by appending the correct extension (e.g. `file:///path/myscript` →
`file:///path/myscript.sh` for `languageId=sh`). If the file already has the correct extension, no rewrite happens.

```jsonc
{
  "lsp_rewrite_uris": true,
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
dprintx fmt --stdin path/to/file.yaml < input.yaml

# fmt/check — groups files by profile, calls dprint per group
dprintx fmt
dprintx check
dprintx fmt file1.go file2.yaml   # explicit file list

# list all files that would be formatted (merged from all profiles)
dprintx output-file-paths

# show which config is used
dprintx config              # all profiles and rules
dprintx config path/to/file # resolved config for a file

# LSP proxy — spawns dprint lsp per profile, routes by file URI
dprintx lsp
```

`dprintx check` exits with code 1 if any files need formatting.

Use `--config <PATH>` to override the config location (default: `~/.config/dprint/dprintx.jsonc`):

```bash
dprintx --config /path/to/custom.jsonc fmt
```

All unknown commands and flags are passed through to the real dprint (`--help`, `-V`, `license`, `completions`, etc.).

## Install

```bash
cargo install --git https://github.com/mocksoul/dprintx
```

### Transparent dprint replacement

Symlink `dprintx` as `dprint` earlier in your `PATH` — it becomes a fully transparent drop-in replacement. All unknown
commands and flags are forwarded to the real dprint binary (configured via `"dprint"` in `dprintx.jsonc`):

```bash
ln -sf ~/.cargo/bin/dprintx ~/.local/bin/dprint
```

Now `dprint fmt`, `dprint check`, `dprint lsp` etc. all go through dprintx automatically. No changes needed in editor
configs, CI scripts, or muscle memory.
