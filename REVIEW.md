# Review guide

Keep this short. The reviewer reads all of it on every run, and short guides get followed.

## What failure looks like here
- A missing or null field in the session JSON on stdin crashes the render or prints an error instead of a status line.
- Errors, warnings, or debug output reach stdout, where Claude Code shows them as the status line.
- A render blocks on network, git, or disk work with no timeout, so the status line lags or goes stale.
- An ANSI escape sequence is left open, or display width is miscounted, so color bleeds or the line wraps.

## Always check
<!-- Bug classes this project has shipped before. -->

## Don't flag
- Style, formatting, naming, or cleanup. Linters and /simplify handle those.
- Missing tests or docs, unless a line above asks for them.

## Intentional patterns
<!-- Deliberate choices the reviewer keeps flagging as bugs, each with the reason. -->

## Evidence
- Cite file:line and the input or path that triggers each finding. Do not guess from a name.
- Flag a bug class once and list every instance, rather than one finding per instance.
