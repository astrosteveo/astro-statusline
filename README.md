# astro-statusline

A status line for [Claude Code](https://code.claude.com/docs/en/statusline), written in Rust.

```text
Opus 5.5 │ astro-statusline │ main │ ctx 63% │ 5h 12% │ 7d 85% │ $1.23
```

From left to right: the model, the current directory, the git branch, how full the context window is,
how much of the 5-hour and 7-day rate limits you've used, and the session's estimated cost. Percentages
are green below 50%, yellow below 80%, and red from 80% up, unless the config moves those points. A
segment is left out until Claude Code sends its data. For example, rate limits appear only for
claude.ai subscribers, after the first response.
A [config file](#configure) changes which segments show and in what order, their colors and text, and
can add bars, graphs, predictions, and more rows.

## Install

```sh
cargo install --path .
```

Then point Claude Code at it in `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR/settings.json`). The
[editor](#edit-in-the-terminal) can do this for you, or add it by hand:

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

## Edit in the terminal

```sh
astro-statusline config
```

opens a full-screen editor for the [config file](#configure), with the status line drawn at the top as
Claude Code would show it, by the same code. The preview changes as you type, before anything is saved.

- **Preview**: `[` and `]` switch between sample sessions: mid-session, a fresh session, near the
  limits, without a subscription (no rate limits), a detached `HEAD`, and outside git. `<` and `>` set
  the terminal width it stands in for, marked with `┊`, to see what drops on a narrow terminal. Glyphs
  that terminals draw at different widths, such as Nerd Font icons, are flagged under it.
- **Presets**: configs to start from: Default (every key at its default, with comments), Plain ASCII
  (for fonts without box drawing), Powerline (needs a Nerd Font), and Dashboard (two rows with bars,
  graphs, and predictions). The preview shows the one you're on, and Enter replaces the config with
  it. Nothing is written until you save.
- **Settings**: pick a group on the left and press Enter to edit a key on the right. A template lists
  its placeholders, and Tab inserts one. A value with a mistake shows what's wrong and isn't kept until
  it's fixed. A style opens a picker: arrow keys choose from the 256 colors, `b` `d` `i` `u` turn
  attributes on and off, `x` clears the color, and `t` types any style, such as `#ff8700`. `r` takes a
  key out of the file, back to its default.
- **Layout**: `J` and `K` (or Shift and an arrow) move a segment within its row, to the next row, or
  into "Not shown". `n` adds a row and `d` removes a row or a segment.
- **Saving** (`s`) changes only what you edited: comments, key order, and formatting stay. A config
  that's a symlink, as dotfile managers make, is written through to its target. The previous file is
  kept as `config.toml.bak`. If the file changed on disk since the editor read it, it asks before
  overwriting. A read-only file, such as one Nix home-manager makes, isn't touched: the editor offers to
  print the TOML when you quit instead. Quitting (`q`) with unsaved changes asks first.
- **Claude Code**: when its settings don't run astro-statusline, this sets `statusLine` to run the
  program you started the editor with, every second. It shows what changes first, keeps every other
  setting, and keeps the previous file as `settings.json.bak`.

A config that isn't valid TOML at all can't be edited key by key, so the editor says where the mistake
is and doesn't open. A config with another kind of mistake, such as an unknown color, opens with the
mistake shown, and saving waits until it's fixed.

## Configure

The config file is `~/.config/astro-statusline/config.toml`, or
`$XDG_CONFIG_HOME/astro-statusline/config.toml` when `XDG_CONFIG_HOME` is set to an absolute path.
Every key is optional. These are the defaults:

```toml
# The segments to show, left to right. Leave one out to hide it.
segments = ["model", "directory", "branch", "context", "five_hour", "seven_day", "cost"]
# Or several rows, top to bottom, instead of segments:
# lines = [["model", "directory", "branch"], ["context", "five_hour", "seven_day", "cost"]]
# "fill" in a row pushes the segments after it to the right edge.

# Segments that drop first when a row is too wide, in this order. The rest drop in the default
# order: cost, seven_day, five_hour, branch, directory, model, and context last.
drop_order = []
# The same number as the padding in Claude Code's statusLine settings, which takes that many
# columns at each end of the line, so a row is fitted to the room that's left.
padding = 0

[colors]
model = "cyan"
directory = "blue"
branch = "magenta"
# usage takes the usage color below, by how much is used.
context = "usage"
five_hour = "usage"
seven_day = "usage"
cost = "dim"
separator = "dim"
# Context and rate-limit percentages: low, medium, and high, as [thresholds] sets them.
usage_low = "green"
usage_medium = "yellow"
usage_high = "red"

# Where each percentage turns from low to medium, and from medium to high.
[thresholds]
context = [50, 80]
five_hour = [50, 80]
seven_day = [50, 80]

# Show a segment only from this value up: a percent for context, five_hour, and seven_day, and
# dollars for cost, such as seven_day = 50 or cost = 1.0. A segment not named here always shows.
[show_from]

# The most columns a segment may take, cut short with … beyond, such as branch = 20. A segment
# not named here takes what it needs.
[max_width]

[format]
# Or a list, one for each row, such as [" │ ", " · "]. Rows past its end take the last.
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
marker = "┃"

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
- `on` and a color, at most once per style, for the background: `"black on cyan"`.

For example, `model = "bold #ff8700"`. An empty string, `""`, draws plain text.

`colors.separator = "powerline"` draws each separator in the backgrounds of the segments on either
side of it, with one more after the last segment of a row, for the arrows of a powerline theme. Set
`format.separator` to the arrow, such as `"\ue0b0"` (U+E0B0, from a Nerd Font), give segments
backgrounds, and put spaces in their templates so the text doesn't touch the arrows:

```toml
[colors]
model = "black on cyan"
directory = "white on blue"
context = "black on usage"
separator = "powerline"

[format]
separator = "\ue0b0"
model = " {name} "
directory = " {name} "
context = " {percent} "
```

`on usage` takes the usage color as the background. A placeholder's own style keeps its segment's
background unless it names one, so the segment stays on its background throughout. With `NO_COLOR`
powerline separators are drawn as plain text, with no closing one.

The context and rate-limit segments take `usage_low`, `usage_medium`, or `usage_high` by how much is
used: their styles default to `usage`, which stands for that usage style. `usage` works in those three
segments' styles and templates only. `[thresholds]` sets where the usage style changes, in whole
percents, for each segment on its own: `context = [60, 85]` draws the context in the low color below
60%, medium from 60%, and high from 85%.

A placeholder can have its own style, written after a colon: `{percent:bold}`. Its value is drawn in
that style, and the rest of the template in the segment's. For a dim label and a bold percentage in
the usage color:

```toml
[colors]
context = "dim"

[format]
context = "ctx {percent:bold usage}"
```

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
| `five_hour`, `seven_day` | `{percent}` `{bar}` `{graph}` `{resets_in}` `{projected}` `{eta}` `{target}` `{delta}` |
| `cost` | `{usd}` `{rate}` `{graph}` |

- `{bar}`: the percentage as a bar of `[bar] width` cells. A cell fills for each whole step. On a rate
  limit, `[bar] marker` marks where `{target}` falls: ahead of the fill, you're under pace; inside it,
  over.
- `{graph}`: the latest values, one cell each, up to `[graph] width`. Percentages use a fixed 0 to 100
  scale. The cost graph shows how much each update added, scaled to the largest one in view.
- `{tokens}`, `{size}`: tokens in the context window, and the most it holds, such as `126k` and `200k`.
- `{resets_in}`: time until the rate-limit window resets, such as `2h13m`.
- `{projected}`: the share of the limit you'll have used when it resets, at your average pace since the
  window started. It waits until 15% of the window has passed.
- `{eta}` on a rate limit: when you'll hit 100% at that pace. It shows only if that comes before the
  reset, and reads `now` once you're there.
- `{target}`: where usage would be by now at an even pace that reaches exactly 100% at the reset, the
  share of the window gone, such as `57%`.
- `{delta}`: how far `{percent}` is over or under `{target}`, such as `+8%` or `-16%`.
- `{eta}` on the context: when it reaches 100% at the rate it grew over the last 15 minutes of work.
  It doesn't count down while you're idle, and auto-compact can come sooner.
- `{rate}`: the session's cost per hour so far, idle time included.

A segment still needs the data it's about: the context segment shows only with a context percentage,
the cost segment only with a cost, and so on, whatever the template says. A value outside brackets
that's missing hides its whole segment. To write a literal brace or bracket, double it: `{{`, `}}`,
`[[`, `]]`.

`[show_from]` hides a segment until its value reaches a number: `seven_day = 50` shows the 7-day
limit from 50% used, and `cost = 1.0` shows the cost from $1. It compares the exact value, before
rounding, so `seven_day = 24` still hides 23.5%, which shows as `24%`.

`[max_width]` cuts a segment to a number of columns, ending in `…`, wherever it is and however wide
the terminal: `branch = 20` draws `feat/a-very-long-br…` for a long branch.

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
seven_day = "7d {bar} {percent}[ ({delta})][ → {projected}]"
cost = "{usd} {graph}[ ({rate}/h)]"

[bar]
width = 8
```

```text
Opus 5.5 · astro-statusline · main
ctx ████░░░░ 58% ▂▃▃▃▄▄▅▅ full in 16m · 5h 71% ⚠ limit in 1h13m resets 2h · 7d ██░┃░░░░ 30% (-13%) → 70% · $1.23 ▃▆▄██▅▄ ($0.82/h)
```

A row with nothing to show is left out, so the second row appears with the first response.

`"fill"` in a row pushes the segments after it to the right edge, with spaces between, as the line
can be no wider than the terminal less Claude Code's edges and `padding`:

```toml
segments = ["model", "directory", "branch", "fill", "context", "cost"]
```

A row has at most one fill. When it's too wide, the fill counts as a separator, so the two sides
never touch. Without `COLUMNS` the fill is drawn as a separator. Claude Code shows notifications on
the status line's row and cuts its right end to make room for them, so a filled row loses its right
side first while a notification shows.

### History

Graphs and the context `{eta}` need to know how values changed, and Claude Code sends only the current
ones. When a segment that's shown uses them, each session keeps a small history file in
`~/.local/state/astro-statusline/sessions/` (or under `$XDG_STATE_HOME`). It's written only when a value
changes, holds the last 64 updates, and is deleted after 7 days without use. Without a graph or a
context `{eta}` in the config, nothing is written.

## Behavior

- **Narrow terminals**: Claude Code passes the terminal width in `COLUMNS`. When a row doesn't fit,
  segments drop in this order: cost, 7-day, 5-hour, branch, directory, model. Context goes last. The
  order holds wherever the config puts them, and each row is fitted on its own. `drop_order` names
  segments to drop first, in its order, ahead of the rest: `drop_order = ["branch"]` drops the branch
  before the cost. A row gets the terminal width less 4 columns, which Claude Code keeps at its edges.
  If you set `padding` in Claude Code's `statusLine` settings, set the same `padding` here: Claude
  Code takes that many columns at each end, and the config can't read it from there.
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
