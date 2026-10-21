# astro-statusline

A status line for [Claude Code](https://code.claude.com/docs/en/statusline), written in Rust.

```text
Opus 5.5 │ astro-statusline │ main │ ctx 63% │ 5h 12% │ 7d 85% │ $1.23
```

From left to right: the model, the current directory, the git branch, how full the context window is,
how much of the 5-hour and 7-day rate limits you've used, and the session's estimated cost. Percentages
are green below 50%, yellow below 80%, and red from 80% up. A segment is left out until Claude Code sends
its data. For example, rate limits appear only for claude.ai subscribers, after the first response.

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
    "refreshInterval": 2
  }
}
```

Claude Code re-runs the command after events like a new message, but not when the terminal is resized.
`refreshInterval` re-runs it every two seconds as well, so the line fits again shortly after a resize. A
run takes about half a millisecond.

## Behavior

- **Narrow terminals**: Claude Code passes the terminal width in `COLUMNS`. When the line doesn't fit,
  segments drop in this order: cost, 7-day, 5-hour, branch, directory, model. Context goes last.
- **Color**: set `NO_COLOR` to any non-empty value to turn colors off.
- **Git**: the branch comes from the repository's `HEAD` file, so no `git` process runs. Linked
  worktrees and submodules work. Repositories that use the reftable ref format show no branch.
- **Bad input**: missing, null, or mistyped fields leave out their segment. The program always exits 0
  and never writes errors to stdout.

## Try it

```sh
echo '{"model":{"display_name":"Opus"},"workspace":{"current_dir":"'"$PWD"'"},"context_window":{"used_percentage":25}}' \
  | cargo run -q
```
