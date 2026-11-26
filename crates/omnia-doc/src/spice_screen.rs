//! `PK10` screen for embedded SPICE text and subcircuit port counting (`P1.2`).
//!
//! Embedded text is what a document or command stores: a deck already extracted and flattened by
//! `omnia-parts` (`PD3`). It is therefore held to the stored form, and any deviation is refused:
//!
//! - no byte-order mark anywhere (extraction drops the leading one), no carriage return (CRLF
//!   became LF at extraction; a lone CR is a line break to some simulators and would let a
//!   directive hide from this line-oriented screen) and no NUL byte;
//! - no `PK10` directive (`.control`, `.endc`, `shell`, `system`, `osdi`, `pre_osdi`,
//!   `codemodel`, `.csdf`) as the first token of any line, continuation lines included;
//! - no `.include`, `.inc`, `.lib` or `.endl` at all: an embedded deck has no external file
//!   references (`PINV9`).
//!
//! Directive tokens are compared ASCII case-insensitively with any leading dots ignored. A
//! token that starts with a dot is also refused when it merely *starts with* one of these
//! names (`.controlx`, `.library`), because simulators commonly match dot-directives by
//! prefix. Over-refusal is accepted: the screen is hostile by default (`P0.3` gap 9).

use std::fmt;

/// `PK10` directive names, without their leading dot, lowercase.
const FORBIDDEN: [&str; 8] = [
    "control",
    "endc",
    "csdf",
    "shell",
    "system",
    "osdi",
    "pre_osdi",
    "codemodel",
];

/// File-reference directives, without their leading dot, lowercase. Embedded text may not
/// carry any of them (`PK10`, embedded rules).
const INCLUDES: [&str; 4] = ["include", "inc", "lib", "endl"];

/// Why embedded SPICE text was refused. Lines are 1-based physical lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenError {
    /// A U+FEFF byte-order mark; stored text never carries one (`PD3`).
    ByteOrderMark { line: usize },
    /// A carriage return; stored text uses LF line endings only (`PD3`).
    CarriageReturn { line: usize },
    /// A NUL byte.
    Nul { line: usize },
    /// A `PK10` forbidden directive.
    Forbidden { line: usize, directive: String },
    /// An `.include`/`.inc`/`.lib`/`.endl` directive, which embedded text may not carry.
    Include { line: usize, directive: String },
}

impl ScreenError {
    /// The 1-based physical line the refusal points at.
    pub fn line(&self) -> usize {
        match self {
            ScreenError::ByteOrderMark { line }
            | ScreenError::CarriageReturn { line }
            | ScreenError::Nul { line }
            | ScreenError::Forbidden { line, .. }
            | ScreenError::Include { line, .. } => *line,
        }
    }
}

impl fmt::Display for ScreenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScreenError::ByteOrderMark { line } => {
                write!(f, "line {line}: byte-order mark in embedded SPICE text")
            }
            ScreenError::CarriageReturn { line } => {
                write!(f, "line {line}: carriage return in embedded SPICE text")
            }
            ScreenError::Nul { line } => write!(f, "line {line}: NUL byte in embedded SPICE text"),
            ScreenError::Forbidden { line, directive } => {
                write!(f, "line {line}: forbidden SPICE directive `{directive}`")
            }
            ScreenError::Include { line, directive } => write!(
                f,
                "line {line}: `{directive}` is not allowed in embedded SPICE text"
            ),
        }
    }
}

impl std::error::Error for ScreenError {}

/// Screens embedded SPICE text under the `PK10` embedded rules (`PD10`, `PINV9`).
pub fn screen_embedded(spice: &str) -> Result<(), ScreenError> {
    for (i, raw) in spice.split('\n').enumerate() {
        let line = i + 1;
        if raw.contains('\u{feff}') {
            return Err(ScreenError::ByteOrderMark { line });
        }
        if raw.contains('\r') {
            return Err(ScreenError::CarriageReturn { line });
        }
        if raw.contains('\0') {
            return Err(ScreenError::Nul { line });
        }
        let Some(first) = screened_token(raw) else {
            continue;
        };
        if is_directive(first, &FORBIDDEN) {
            return Err(ScreenError::Forbidden {
                line,
                directive: first.to_owned(),
            });
        }
        if is_directive(first, &INCLUDES) {
            return Err(ScreenError::Include {
                line,
                directive: first.to_owned(),
            });
        }
    }
    Ok(())
}

/// The token the screen checks on one physical line: the first token of a normal line, or the
/// first token after the `+` of a continuation line. Comment (`*`) and blank lines have none.
/// Tokens split on any Unicode whitespace, so `shell\u{a0}x` still yields `shell`.
fn screened_token(raw: &str) -> Option<&str> {
    let t = raw.trim_start();
    if t.starts_with('*') {
        return None;
    }
    let body = t.strip_prefix('+').unwrap_or(t);
    code_part(body)
        .split(char::is_whitespace)
        .find(|s| !s.is_empty())
}

/// Whether `token` names one of `names`: exact after dropping leading dots, or, for a token
/// written with a leading dot, a prefix match.
fn is_directive(token: &str, names: &[&str]) -> bool {
    let bare = token.trim_start_matches('.');
    let dotted = bare.len() != token.len();
    names.iter().any(|name| {
        bare.eq_ignore_ascii_case(name)
            || (dotted
                && bare.len() > name.len()
                && bare.is_char_boundary(name.len())
                && bare[..name.len()].eq_ignore_ascii_case(name))
    })
}

/// Cuts an inline comment: `;` anywhere, `$` or `//` after whitespace (or at the start), with
/// quoted text skipped (`P0.3` gap 6).
fn code_part(s: &str) -> &str {
    let b = s.as_bytes();
    let mut in_quote = false;
    for i in 0..b.len() {
        let c = b[i];
        if c == b'"' {
            in_quote = !in_quote;
            continue;
        }
        if in_quote {
            continue;
        }
        let prev_ws = i == 0 || b[i - 1].is_ascii_whitespace();
        match c {
            b';' => return &s[..i],
            b'$' if prev_ws && (i + 1 == b.len() || b[i + 1].is_ascii_whitespace()) => {
                return &s[..i];
            }
            b'/' if prev_ws && b.get(i + 1) == Some(&b'/') => return &s[..i],
            _ => {}
        }
    }
    s
}

/// Whitespace tokens with `a = b`, `a= b` and `a =b` merged into `a=b` (`P0.3` gap 5).
fn tokens(code: &str) -> Vec<String> {
    let raw: Vec<&str> = code.split_ascii_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let t = raw[i];
        if t == "=" && !out.is_empty() && i + 1 < raw.len() {
            let last = out.pop().unwrap_or_default();
            out.push(format!("{last}={}", raw[i + 1]));
            i += 2;
        } else if t.ends_with('=') && t.len() > 1 && i + 1 < raw.len() {
            out.push(format!("{t}{}", raw[i + 1]));
            i += 2;
        } else if t.starts_with('=') && t.len() > 1 && !out.is_empty() {
            let last = out.pop().unwrap_or_default();
            out.push(format!("{last}{t}"));
            i += 1;
        } else {
            out.push(t.to_owned());
            i += 1;
        }
    }
    out
}

/// The number of ports of the top-level `.subckt` named `entry` (ASCII case-insensitive), or
/// `None` if there is none. The header is read as one logical line: `+` continuations are
/// joined, with blank and `*` comment lines allowed between them, and inline comments cut.
/// Ports end at `PARAMS:` (any case) or at the first `name=value`, after `a = b` is merged into
/// `a=b`. Used for terminal counts (`PINV3`, `PINV11`).
pub fn subckt_ports(spice: &str, entry: &str) -> Option<usize> {
    let mut depth = 0usize;
    for logical in logical_lines(spice) {
        let toks = tokens(&logical);
        let Some(head) = toks.first() else {
            continue;
        };
        if head.eq_ignore_ascii_case(".subckt") {
            if depth == 0 && toks.get(1).is_some_and(|n| n.eq_ignore_ascii_case(entry)) {
                let rest = &toks[2..];
                let ports = rest
                    .iter()
                    .position(|t| {
                        t.get(..7)
                            .is_some_and(|p| p.eq_ignore_ascii_case("params:"))
                            || t.contains('=')
                    })
                    .unwrap_or(rest.len());
                return Some(ports);
            }
            depth += 1;
        } else if head.eq_ignore_ascii_case(".ends") {
            depth = depth.saturating_sub(1);
        }
    }
    None
}

/// Joined logical lines (code only, comments cut). A continuation with no line to continue
/// starts a logical line of its own.
fn logical_lines(spice: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in spice.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let t = raw.trim_start();
        if t.starts_with('*') {
            continue;
        }
        if let Some(body) = t.strip_prefix('+') {
            let code = code_part(body).trim();
            match out.last_mut() {
                Some(cur) => {
                    cur.push(' ');
                    cur.push_str(code);
                }
                None => out.push(code.to_owned()),
            }
            continue;
        }
        let code = code_part(t).trim();
        if !code.is_empty() {
            out.push(code.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinv9_every_pk10_directive_is_refused_in_embedded_text() {
        for d in [
            ".control",
            ".endc",
            "shell",
            "system",
            "osdi",
            "pre_osdi",
            "codemodel",
            ".csdf",
        ] {
            for written in [d.to_owned(), d.to_ascii_uppercase(), format!(".{d}")] {
                let text = format!(".model D1 D(IS=1e-14)\n  {written} arg\n");
                assert_eq!(
                    screen_embedded(&text),
                    Err(ScreenError::Forbidden {
                        line: 2,
                        directive: written.clone()
                    }),
                    "{written}"
                );
            }
        }
    }

    #[test]
    fn pinv9_forbidden_directive_on_a_continuation_is_refused() {
        let text = ".model D1 D(IS=1e-14\n+ shell rm\n+ N=1)\n";
        assert!(matches!(
            screen_embedded(text),
            Err(ScreenError::Forbidden { line: 2, .. })
        ));
    }

    #[test]
    fn pinv9_dot_directives_match_by_prefix_and_unicode_space_splits() {
        for text in [
            ".CONTROLX\n",
            ".endc;\n",
            ".control\u{a0}x\n",
            "\u{a0}.control\n",
        ] {
            assert!(
                matches!(screen_embedded(text), Err(ScreenError::Forbidden { .. })),
                "{text:?}"
            );
        }
        assert!(matches!(
            screen_embedded("shell\u{a0}rm -rf\n"),
            Err(ScreenError::Forbidden { .. })
        ));
    }

    #[test]
    fn pinv9_includes_are_refused_in_embedded_text() {
        for line in [
            ".include x.lib",
            ".INC x.lib",
            ".inc \"a b.lib\"",
            ".lib corners.lib TT",
            ".lib TT",
            ".endl TT",
            ".library x",
        ] {
            let text = format!("* header\n{line}\n");
            assert!(
                matches!(
                    screen_embedded(&text),
                    Err(ScreenError::Include { line: 2, .. })
                ),
                "{line}"
            );
        }
    }

    #[test]
    fn pd3_stored_form_rules() {
        assert_eq!(
            screen_embedded("\u{feff}.model D1 D\n"),
            Err(ScreenError::ByteOrderMark { line: 1 })
        );
        assert_eq!(
            screen_embedded(".model D1 D\r\n"),
            Err(ScreenError::CarriageReturn { line: 1 })
        );
        // A lone CR would be a line break to some simulators: `.control` must not hide behind it.
        assert_eq!(
            screen_embedded("* note\r.control\n"),
            Err(ScreenError::CarriageReturn { line: 1 })
        );
        assert_eq!(
            screen_embedded(".model D1 D\n*\0\n"),
            Err(ScreenError::Nul { line: 2 })
        );
    }

    #[test]
    fn comments_and_ordinary_text_pass() {
        let text = "* .control in a comment is fine\n\
                    .subckt AMP in out PARAMS: gain=2 ; .include in a comment\n\
                    E1 out 0 in 0 {gain} $ shell\n\
                    + ; continuation comment\n\
                    .ends AMP\n\
                    .model shellish D\n\
                    \n";
        assert_eq!(screen_embedded(text), Ok(()));
        assert_eq!(screen_embedded(""), Ok(()));
    }

    #[test]
    fn subckt_ports_counts_header_ports() {
        let text = ".SUBCKT Opamp inp inn vcc vee out\n.ends\n";
        assert_eq!(subckt_ports(text, "OPAMP"), Some(5));
        assert_eq!(subckt_ports(text, "opamp"), Some(5));
        assert_eq!(subckt_ports(text, "other"), None);
    }

    #[test]
    fn subckt_ports_joins_continuations_with_comments_between() {
        let text = ".subckt XTAL\n\
                    * ports on the next lines\n\
                    + a\n\
                    \n\
                    + b ; second port\n\
                    + params: c0=5p\n\
                    C0 a b {c0}\n\
                    .ends XTAL\n";
        assert_eq!(subckt_ports(text, "xtal"), Some(2));
    }

    #[test]
    fn subckt_ports_stops_at_params_or_first_assignment() {
        assert_eq!(
            subckt_ports(".subckt A p n PARAMS: g=1\n.ends\n", "A"),
            Some(2)
        );
        assert_eq!(
            subckt_ports(".subckt A p n PARAMS:g=1\n.ends\n", "A"),
            Some(2)
        );
        assert_eq!(subckt_ports(".subckt A p n g = 1\n.ends\n", "A"), Some(2));
        assert_eq!(subckt_ports(".subckt A p n g =1\n.ends\n", "A"), Some(2));
        assert_eq!(subckt_ports(".subckt A\n.ends\n", "A"), Some(0));
    }

    #[test]
    fn subckt_ports_ignores_nested_subcircuits_of_the_same_name() {
        let text = ".subckt OUTER a b\n\
                    .subckt INNER x\n\
                    .ends INNER\n\
                    .ends OUTER\n\
                    .subckt INNER p q r\n\
                    .ends INNER\n";
        assert_eq!(subckt_ports(text, "inner"), Some(3));
        assert_eq!(subckt_ports(text, "outer"), Some(2));
    }
}
