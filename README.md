# astro-statusline

A status line for [Claude Code](https://code.claude.com/docs/en/statusline), written in Rust.

```text
Opus 5.5 │ astro-statusline │ main │ ctx 63% │ 5h 12% │ 7d 85% │ $1.23
```

From left to right: the model, the current directory, the git branch, how full the context window is,
how much of the 5-hour and 7-day rate limits you've used, and the session's estimated cost. Percentages
are green below 50%, yellow below 80%, and red from 80% up. A segment is left out until Claude Code sends
its data. For example, rate limits appear only for claude.ai subscribers, after the first response.
A [config file](#configure) changes which segments show and in what order, their colors and text, and
can add bars, graphs, predictions, and more rows.

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
`$XDG_CONFIG_HOME/astro-statusline/config.toml` when `XDG_CONFIG_HOME` is set to an absolute path.
Every key is optional. These are the defaults:

```toml
# The segments to show, left to right. Leave one out to hide it.
segments = ["model", "directory", "branch", "context", "five_hour", "seven_day", "cost"]
# Or several rows, top to bottom, instead of segments:
# lines = [["model", "directory", "branch"], ["context", "five_hour", "seven_day", "cost"]]

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

[format]
separator = " │ "
model = "{name}"
directory = "{name}"
branch = "{branch}"
context = "ctx {percent}"
five_hour = "5h {percent}"
seven_day = "7d {percent}"
cost = "{usd}"

[bar]
width = 10
full = "█"
empty = "░"

[graph]
width = 8
chars = "▁▂▃▄▅▆▇█"
```

Changes apply the next time the line refreshes. If the file has a mistake, such as an unknown key,
segment, color, or placeholder, the whole file is ignored and the line uses the defaults. To see the
error, run the command by hand:

```sh
astro-statusline </dev/null
```

### Colors

A style is words separated by spaces, each one of these:

- An attribute: `bold`, `dim`, `italic`, or `underline`.
- A color, at most one per style: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, or
  `white`, a `bright_` version of any of those such as `bright_red`, a 256-color number from `0` to
  `255` (quoted, as in `"208"`), or `"#rrggbb"`.

For example, `model = "bold #ff8700"`. An empty string, `""`, draws plain text.

### Formats

Each segment's text comes from its template in `[format]`. `{name}` is replaced by a value, and text
in `[` and `]` shows only when every value inside it is there. Put predictions in brackets, since
they come and go:

| Segment | Placeholders |
|---|---|
| `model` | `{name}` |
| `directory` | `{name}` |
| `branch` | `{branch}` |
| `context` | `{percent}` `{bar}` `{graph}` `{eta}` `{tokens}` `{size}` |
| `five_hour`, `seven_day` | `{percent}` `{bar}` `{graph}` `{resets_in}` `{projected}` `{eta}` |
| `cost` | `{usd}` `{rate}` `{graph}` |

- `{bar}`: the percentage as a bar of `[bar] width` cells. A cell fills for each whole step.
- `{graph}`: the latest values, one cell each, up to `[graph] width`. Percentages use a fixed 0 to 100
  scale. The cost graph shows how much each update added, scaled to the largest one in view.
- `{tokens}`, `{size}`: tokens in the context window, and the most it holds, such as `126k` and `200k`.
- `{resets_in}`: time until the rate-limit window resets, such as `2h13m`.
- `{projected}`: the share of the limit you'll have used when it resets, at your average pace since the
  window started. It waits until 15% of the window has passed.
- `{eta}` on a rate limit: when you'll hit 100% at that pace. It shows only if that comes before the
  reset, and reads `now` once you're there.
- `{eta}` on the context: when it reaches 100% at the rate it grew over the last 15 minutes of work.
  It doesn't count down while you're idle, and auto-compact can come sooner.
- `{rate}`: the session's cost per hour so far, idle time included.

A segment still needs the data it's about: the context segment shows only with a context percentage,
the cost segment only with a cost, and so on, whatever the template says. A value outside brackets
that's missing hides its whole segment. To write a literal brace or bracket, double it: `{{`, `}}`,
`[[`, `]]`.

For example, two rows with bars, graphs, and predictions:

```toml
lines = [
  ["model", "directory", "branch"],
  ["context", "five_hour", "seven_day", "cost"],
]

[format]
separator = " · "
context = "ctx {bar} {percent} {graph}[ full in {eta}]"
five_hour = "5h {percent}[ ⚠ limit in {eta}][ resets {resets_in}]"
seven_day = "7d {percent}[ → {projected}]"
cost = "{usd} {graph}[ ({rate}/h)]"

[bar]
width = 8
```

```text
Opus 5.5 · astro-statusline · main
ctx ████░░░░ 58% ▂▃▃▃▄▄▅▅ full in 16m · 5h 71% ⚠ limit in 1h13m resets 2h · 7d 30% → 70% · $1.23 ▃▆▄██▅▄ ($0.82/h)
```

A row with nothing to show is left out, so the second row appears with the first response.

### History

Graphs and the context `{eta}` need to know how values changed, and Claude Code sends only the current
ones. When a segment that's shown uses them, each session keeps a small history file in
`~/.local/state/astro-statusline/sessions/` (or under `$XDG_STATE_HOME`). It's written only when a value
changes, holds the last 64 updates, and is deleted after 7 days without use. Without a graph or a
context `{eta}` in the config, nothing is written.

## Behavior

- **Narrow terminals**: Claude Code passes the terminal width in `COLUMNS`. When a row doesn't fit,
  segments drop in this order: cost, 7-day, 5-hour, branch, directory, model. Context goes last. The
  order holds wherever the config puts them, and each row is fitted on its own.
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
