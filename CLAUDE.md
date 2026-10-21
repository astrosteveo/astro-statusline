# astro-statusline

A statusline engine for Claude Code.

<!-- Every line must be true and checkable: name the file, command, or pattern that shows it. No absolute rule the
code can't meet. Keep it short, and point to other docs instead of copying them. Delete this comment when done. -->

## Stack
- Rust, edition 2024: one binary crate, `Cargo.toml` at the repo root, code in `src/`.

## Check
`cargo clippy --all-targets -- -D warnings && cargo test`

## More
- REVIEW.md: review settings and guide for /solo-review:review.
