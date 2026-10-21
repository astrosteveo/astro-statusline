# astro-statusline

A status line for [Claude Code](https://code.claude.com/docs/en/statusline), written in Rust.

```text
Opus 5.5 │ astro-statusline │ main │ ctx 63% │ 5h 12% │ 7d 85% │ $1.23
```

From left to right: the model, the current directory, the git branch, how full the context window is,
how much of the 5-hour and 7-day rate limits you've used, and the session's estimated cost. Percentages
are green below 50%, yellow below 80%, and red from 80% up. A segment is left out until Claude Code sends
its data. For example, rate limits appear only for claude.ai subscribers, after the first response.
A [config file](#configure) changes which segments show, their order, and their colors.

## Install

```sh
cargo install --path .
```

Then point Claude Code at it in `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.cargo/bin/astro-statusline",
    "refreshInterval": 1
  }
}
```

Claude Code re-runs the command after events like a new message, but not when the terminal is resized.
`refreshInterval` also re-runs it every second, the shortest interval Claude Code allows, so the line
fits again within a second of a resize. A run takes about half a millisecond.

## Configure

The config file is `~/.config/astro-statusline/config.toml`, or
`$XDG_CONFIG_HOME/astro-statusline/config.toml` when `XDG_CONFIG_HOME` is set to an absolute path. Every key is optional.
These are the defaults:

```toml
# The segments to show, left to right. Leave one out to hide it.
segments = ["model", "directory", "branch", "context", "five_hour", "seven_day", "cost"]

[colors]
model = "cyan"
directory = "blue"
branch = "magenta"
cost = "dim"
separator = "dim"
# Context and rate-limit percentages: below 50%, below 80%, and from 80% up.
usage_low = "green"
usage_medium = "yellow"
usage_high = "red"
```

A style is words separated by spaces, each one of these:

- An attribute: `bold`, `dim`, `italic`, or `underline`.
- A color, at most one per style: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, or
  `white`, a `bright_` version of any of those such as `bright_red`, a 256-color number from `0` to
  `255` (quoted, as in `"208"`), or `"#rrggbb"`.

For example, `model = "bold #ff8700"`. An empty string, `""`, draws plain text.

Changes apply the next time the line refreshes. If the file has a mistake, such as an unknown key,
segment, or color, the whole file is ignored and the line uses the defaults. To see the error, run the
command by hand:

```sh
astro-statusline </dev/null
```

## Behavior

- **Narrow terminals**: Claude Code passes the terminal width in `COLUMNS`. When the line doesn't fit,
  segments drop in this order: cost, 7-day, 5-hour, branch, directory, model. Context goes last. The
  order holds wherever the config puts them.
- **Color**: set `NO_COLOR` to any non-empty value to turn colors off, whatever the config says.
- **Git**: the branch comes from the repository's `HEAD` file, so no `git` process runs. Linked
  worktrees and submodules work. Repositories that use the reftable ref format show no branch.
- **Bad input**: missing, null, or mistyped fields leave out their segment. The program always exits 0
  and never writes errors to stdout.

## Try it

```sh
echo '{"model":{"display_name":"Opus"},"workspace":{"current_dir":"'"$PWD"'"},"context_window":{"used_percentage":25}}' \
  | cargo run -q
```
