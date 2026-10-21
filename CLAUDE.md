# astro-statusline

A statusline engine for Claude Code.

<!-- Every line must be true and checkable: name the file, command, or pattern that shows it. No absolute rule the
code can't meet. Keep it short, and point to other docs instead of copying them. Delete this comment when done. -->

## Stack
- Rust, edition 2024: one binary crate, `Cargo.toml` at the repo root, code in `src/`.

## Check
`cargo clippy --all-targets -- -D warnings && cargo test`

## solo-review
- This is the first project scaffolded and built with the solo-review plugin (REVIEW.md, `.review/`,
  `.github/pull_request_template.md`).
- When you hit a problem or edge case in solo-review, or have a suggestion, improvement, or enhancement for it, open
  a GitHub issue at `astrosteveo/astro-plugins` (`gh issue create --repo astrosteveo/astro-plugins`).

## More
- REVIEW.md: review settings and guide for /solo-review:review.
