---
# solo-review settings. Every key is optional; these are the defaults.
depth: auto            # auto | quick | standard | deep
security: auto         # auto | always | never
check: "cargo clippy --all-targets -- -D warnings && cargo test"
max_nits: 3
max_rounds: 3          # after this many rounds on one change, step back instead of re-reviewing
gate: push             # push | off: block Claude's `git push` while Important findings are open
branch: require        # require | off: no file edits on the default branch
workflow: manual       # manual | auto: auto runs review and ship when a task is done; manual waits for you to ask
merge: rebase          # rebase | squash | merge: how /solo-review:ship lands a branch
sensitive:             # reviewed deep with a security pass, and you must read them before a merge. Added to the
                       # plugin's always-sensitive list (Claude config, CLAUDE.md, CI, git hooks, dependency
                       # manifests, secrets), which nothing here can remove. `ledger.py config` shows both.
  - "**/install.sh"
  - "**/release.sh"
  - "**/auth/**"
skip:                  # never reviewed, except files on the always-sensitive list (such as lockfiles), which
                       # skip never hides
  - "**/*.min.js"
  - "**/*.snap"
---

# Review guide

Keep this short. The reviewer reads all of it on every run, and short guides get followed.

## What failure looks like here
- A missing or null field in the session JSON on stdin crashes the render or prints an error instead of a status line.
- Errors, warnings, or debug output reach stdout, where Claude Code shows them as the status line.
- A render blocks on network, git, or disk work with no timeout, so the status line lags or goes stale.
- An ANSI escape sequence is left open, or display width is miscounted, so color bleeds or the line wraps.

## Always check
<!-- Bug classes this project has shipped before. /solo-review:review-learn adds to this list. -->

## Don't flag
- Style, formatting, naming, or cleanup. Linters and /simplify handle those.
- Missing tests or docs, unless a line above asks for them.

## Intentional patterns
<!-- Deliberate choices the reviewer keeps flagging as bugs, each with the reason. -->

## Evidence
- Cite file:line and the input or path that triggers each finding. Do not guess from a name.
- Flag a bug class once and list every instance, rather than one finding per instance.
