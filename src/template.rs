//! Format templates: a segment's text, with `{placeholder}`s for its values and `[...]` groups
//! that show only when every placeholder inside them has a value.
//!
//! Braces read left to right, as in Python's `str.format`: `{{` and `}}` are literal braces, so
//! `{{{x}}}` draws `{`, then `x`, then `}`. In a run of brackets, pairs are literal brackets and an
//! odd one out belongs to the group: the first `[` of a run opens it and the last `]` of a run
//! closes it, so `[[[{x}]]]` is a group that draws `[x]`. Values are inserted as plain text and
//! never parsed again.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template(Vec<Part>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Piece(Piece),
    Group(Vec<Piece>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Value(String),
}

impl Template {
    /// Parses `text`, allowing only the placeholders in `allowed`.
    pub fn parse(text: &str, allowed: &[&str]) -> Result<Self, String> {
        let chars: Vec<char> = text.chars().collect();
        let mut parts = Vec::new();
        let mut group: Option<Vec<Piece>> = None;
        let mut literal = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let run = chars[i..].iter().take_while(|&&next| next == c).count();
            match c {
                '{' => {
                    literal.extend(std::iter::repeat_n('{', run / 2));
                    i += run;
                    if run % 2 == 1 {
                        let name_len = chars[i..].iter().take_while(|&&c| c != '}').count();
                        if i + name_len == chars.len() {
                            return Err(format!("{text:?} has a {{ with no }}"));
                        }
                        let name: String = chars[i..i + name_len].iter().collect();
                        i += name_len + 1;
                        check_name(&name, allowed)?;
                        flush(&mut literal, &mut parts, &mut group);
                        push(&mut parts, &mut group, Piece::Value(name));
                    }
                }
                '}' => {
                    if run % 2 == 1 {
                        return Err(format!(
                            "{text:?} has a }} with no {{; write }}}} for a literal brace"
                        ));
                    }
                    literal.extend(std::iter::repeat_n('}', run / 2));
                    i += run;
                }
                '[' => {
                    if run % 2 == 1 {
                        if group.is_some() {
                            return Err(format!("{text:?} has a [ group inside another"));
                        }
                        flush(&mut literal, &mut parts, &mut group);
                        group = Some(Vec::new());
                    }
                    literal.extend(std::iter::repeat_n('[', run / 2));
                    i += run;
                }
                ']' => {
                    literal.extend(std::iter::repeat_n(']', run / 2));
                    i += run;
                    if run % 2 == 1 {
                        if group.is_none() {
                            return Err(format!(
                                "{text:?} has a ] with no [; write ]] for a literal bracket"
                            ));
                        }
                        flush(&mut literal, &mut parts, &mut group);
                        let inner = group.take().unwrap_or_default();
                        if !inner.iter().any(|piece| matches!(piece, Piece::Value(_))) {
                            return Err(format!(
                                "{text:?} has a [ ] group with no placeholder in it; write [[ and \
                                 ]] for literal brackets"
                            ));
                        }
                        parts.push(Part::Group(inner));
                    }
                }
                _ => {
                    literal.push(c);
                    i += 1;
                }
            }
        }
        if group.is_some() {
            return Err(format!("{text:?} has a [ with no ]"));
        }
        flush(&mut literal, &mut parts, &mut group);
        Ok(Template(parts))
    }

    /// Fills the template with `value(name)` for each placeholder. `None` when a placeholder
    /// outside any group has no value. An empty value counts as none.
    pub fn render(&self, value: impl Fn(&str) -> Option<String>) -> Option<String> {
        let mut out = String::new();
        for part in &self.0 {
            match part {
                Part::Piece(piece) => {
                    out.push_str(&render_pieces(std::slice::from_ref(piece), &value)?)
                }
                Part::Group(pieces) => {
                    if let Some(text) = render_pieces(pieces, &value) {
                        out.push_str(&text);
                    }
                }
            }
        }
        Some(out)
    }
}

/// Renders `pieces`, or `None` if any placeholder among them has no value.
fn render_pieces(pieces: &[Piece], value: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Text(text) => out.push_str(text),
            Piece::Value(name) => out.push_str(&value(name).filter(|v| !v.is_empty())?),
        }
    }
    Some(out)
}

fn check_name(name: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&name) {
        return Ok(());
    }
    let expected: Vec<String> = allowed.iter().map(|name| format!("{{{name}}}")).collect();
    Err(format!(
        "unknown placeholder {{{name}}}: expected {}",
        expected.join(", ")
    ))
}

/// Adds `piece` to the open group, or to the template itself when no group is open.
fn push(parts: &mut Vec<Part>, group: &mut Option<Vec<Piece>>, piece: Piece) {
    match group {
        Some(pieces) => pieces.push(piece),
        None => parts.push(Part::Piece(piece)),
    }
}

fn flush(literal: &mut String, parts: &mut Vec<Part>, group: &mut Option<Vec<Piece>>) {
    if !literal.is_empty() {
        push(parts, group, Piece::Text(std::mem::take(literal)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOWED: &[&str] = &["x", "eta", "resets_in"];

    fn render(template: &str, values: &[(&str, &str)]) -> Option<String> {
        Template::parse(template, ALLOWED).unwrap().render(|name| {
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        })
    }

    fn error(template: &str) -> String {
        Template::parse(template, ALLOWED).unwrap_err()
    }

    #[test]
    fn fills_placeholders_between_text() {
        assert_eq!(
            render("ctx {x}!", &[("x", "63%")]).as_deref(),
            Some("ctx 63%!")
        );
        assert_eq!(render("{x}{x}", &[("x", "a")]).as_deref(), Some("aa"));
        assert_eq!(render("plain", &[]).as_deref(), Some("plain"));
        assert_eq!(render("", &[]).as_deref(), Some(""));
        assert_eq!(
            render("{resets_in}", &[("resets_in", "2h")]).as_deref(),
            Some("2h")
        );
    }

    #[test]
    fn a_missing_value_outside_a_group_drops_everything() {
        assert_eq!(render("ctx {x}", &[]), None);
        assert_eq!(render("ctx {x}", &[("x", "")]), None);
    }

    #[test]
    fn a_group_shows_only_when_all_its_values_are_there() {
        let template = "{x}[ full in {eta}]";
        assert_eq!(
            render(template, &[("x", "63%"), ("eta", "25m")]).as_deref(),
            Some("63% full in 25m")
        );
        assert_eq!(render(template, &[("x", "63%")]).as_deref(), Some("63%"));
        assert_eq!(
            render(template, &[("x", "63%"), ("eta", "")]).as_deref(),
            Some("63%")
        );
        let both = "[{x} {eta}]";
        assert_eq!(render(both, &[("x", "a")]).as_deref(), Some(""));
        assert_eq!(
            render(both, &[("x", "a"), ("eta", "b")]).as_deref(),
            Some("a b")
        );
    }

    #[test]
    fn doubled_braces_and_brackets_are_literal() {
        assert_eq!(render("{{{x}}}", &[("x", "a")]).as_deref(), Some("{a}"));
        assert_eq!(render("{{x}}", &[]).as_deref(), Some("{x}"));
        assert_eq!(render("[[beta]]", &[]).as_deref(), Some("[beta]"));
        assert_eq!(render("[[[{x}]]]", &[("x", "a")]).as_deref(), Some("[a]"));
        // The literal brackets belong to the group, so they go with it.
        assert_eq!(render("ctx[[[{x}]]]", &[]).as_deref(), Some("ctx"));
        assert_eq!(render("[{x}]]]", &[("x", "a")]).as_deref(), Some("a]"));
    }

    #[test]
    fn values_are_never_parsed_as_templates() {
        assert_eq!(
            render("{x}", &[("x", "{eta} [x] }{")]).as_deref(),
            Some("{eta} [x] }{")
        );
    }

    #[test]
    fn rejects_unknown_and_malformed_placeholders() {
        assert_eq!(
            error("{nope}"),
            "unknown placeholder {nope}: expected {x}, {eta}, {resets_in}"
        );
        assert!(error("{}").starts_with("unknown placeholder {}"));
        assert!(error("{ x }").starts_with("unknown placeholder { x }"));
        assert!(error("{X}").starts_with("unknown placeholder {X}"));
        assert_eq!(error("ctx {x"), r#""ctx {x" has a { with no }"#);
        assert!(error("x}").contains("has a } with no {"));
        assert!(
            error("{x}}}}").contains("has a } with no {"),
            "{}",
            error("{x}}}}")
        );
    }

    #[test]
    fn rejects_malformed_groups() {
        assert!(error("[{x}").contains("has a [ with no ]"));
        assert!(error("{x}]").contains("has a ] with no ["));
        assert!(error("[a [{x}] b]").contains("inside another"));
        assert!(error("[beta]").contains("no placeholder in it"));
        assert!(error("[]").contains("no placeholder in it"));
    }
}
