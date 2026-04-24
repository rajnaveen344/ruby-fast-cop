//! Style/RedundantRegexpArgument - Replace deterministic regexp args with string args.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_regexp_argument.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

const RESTRICT_ON_SEND: &[&str] = &[
    "byteindex",
    "byterindex",
    "gsub",
    "gsub!",
    "partition",
    "rpartition",
    "scan",
    "split",
    "start_with?",
    "sub",
    "sub!",
];

/// Special string chars — when present in replacement, emit as double-quoted
/// preserving the escape. Matches RuboCop's STR_SPECIAL_CHARS (chars that have
/// meaning inside double-quoted strings).
const STR_SPECIAL_CHARS: &[&str] = &[
    r"\a", r"\c", r"\C", r"\e", r"\f", r"\M", r"\n", "\\\"", "\\'", r"\\", r"\t", r"\b", r"\f",
    r"\r", r"\u", r"\v", r"\x", r"\0", r"\1", r"\2", r"\3", r"\4", r"\5", r"\6", r"\7",
];

pub struct RedundantRegexpArgument {
    enforce_double_quotes: bool,
}

impl Default for RedundantRegexpArgument {
    fn default() -> Self {
        Self {
            enforce_double_quotes: false,
        }
    }
}

impl RedundantRegexpArgument {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(enforce_double_quotes: bool) -> Self {
        Self { enforce_double_quotes }
    }
}

/// Literal regex char set: `[\w\s\-,"'!#%&<>=;:\`~/]`.
fn is_literal_char(c: char) -> bool {
    c.is_alphanumeric()
        || c == '_'
        || c.is_whitespace()
        || matches!(
            c,
            '-' | ',' | '"' | '\'' | '!' | '#' | '%' | '&' | '<' | '>' | '=' | ';' | ':' | '`' | '~' | '/'
        )
}

/// Chars that CAN'T follow a backslash: AbBdDgGhHkpPRwWXsSzZ + digit.
fn is_non_literal_escape(c: char) -> bool {
    matches!(
        c,
        'A' | 'b' | 'B' | 'd' | 'D' | 'g' | 'G' | 'h' | 'H' | 'k' | 'p' | 'P' | 'R' | 'w' | 'W' | 'X' | 's' | 'S' | 'z' | 'Z'
    ) || c.is_ascii_digit()
}

/// Test whether the regexp *source* (including `/.../`) is deterministic:
/// matches `\A(?:LITERAL_REGEX)+\Z`. Our source is always `/.../` form.
fn deterministic(source: &str) -> bool {
    // Strip opening `/` and closing `/` (no flags — caller checks regopt empty).
    let mut chars = source.chars().peekable();
    if chars.next() != Some('/') {
        return false;
    }
    let inner: String = chars.by_ref().collect();
    if !inner.ends_with('/') {
        return false;
    }
    let body = &inner[..inner.len() - 1];
    if body.is_empty() {
        return false;
    }

    let mut it = body.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            let next = match it.next() {
                Some(n) => n,
                None => return false,
            };
            if is_non_literal_escape(next) {
                return false;
            }
        } else if !is_literal_char(c) {
            return false;
        }
    }
    true
}

/// RuboCop's `replacement`: unescape each literal char. If escape char is a
/// string-special char (like `\n`), keep the escape; otherwise strip the `\`.
fn replacement_from_content(content: &str) -> String {
    // Ruby logic: groups chars, pairing a preceding \ with the next char.
    let mut tokens: Vec<String> = Vec::new();
    let mut pending_backslash = false;
    for c in content.chars() {
        if !pending_backslash && c == '\\' {
            pending_backslash = true;
        } else {
            let mut tok = String::new();
            if pending_backslash {
                tok.push('\\');
                pending_backslash = false;
            }
            tok.push(c);
            tokens.push(tok);
        }
    }
    // Flush trailing backslash if any (shouldn't happen for deterministic).
    if pending_backslash {
        tokens.push("\\".to_string());
    }

    tokens
        .into_iter()
        .map(|tok| {
            if STR_SPECIAL_CHARS.iter().any(|s| *s == tok) {
                tok
            } else {
                tok.replace('\\', "")
            }
        })
        .collect()
}

/// Build the preferred string literal from the (already unescaped) argument.
fn preferred_argument(mut arg: String, enforce_double_quotes: bool) -> String {
    let quote;
    if arg.contains('"') {
        // Replace ' with \\'
        arg = arg.replace('\'', "\\'");
        // Replace \" with "
        arg = arg.replace("\\\"", "\"");
        quote = '\'';
    } else if arg.contains("\\'") {
        // Add backslash before single-quotes preceded by even (incl zero) number of backslashes.
        // Simpler: add backslash to each ' that is NOT preceded by \.
        arg = escape_singles_preserving_escaped(&arg);
        quote = '\'';
    } else if arg.contains('\'') {
        arg = arg.replace('\'', "\\'");
        quote = '\'';
    } else if arg.contains('\\') {
        quote = '"';
    } else {
        quote = if enforce_double_quotes { '"' } else { '\'' };
    }
    format!("{q}{a}{q}", q = quote, a = arg)
}

/// Replace `'` with `\'` but only when preceded by an even number of `\`
/// (meaning the `'` itself is not already escaped).
fn escape_singles_preserving_escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            // Count preceding backslashes in `out`.
            let mut count = 0;
            let out_bytes = out.as_bytes();
            let mut j = out_bytes.len();
            while j > 0 && out_bytes[j - 1] == b'\\' {
                j -= 1;
                count += 1;
            }
            if count % 2 == 0 {
                out.push('\\');
            }
            out.push('\'');
            i += 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

impl Cop for RedundantRegexpArgument {
    fn name(&self) -> &'static str {
        "Style/RedundantRegexpArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if !RESTRICT_ON_SEND.iter().any(|m| *m == method.as_ref()) {
            return vec![];
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return vec![];
        }

        let first_arg = &arg_list[0];
        let re = match first_arg.as_regular_expression_node() {
            Some(r) => r,
            None => return vec![],
        };

        // Must have no flags — closing source = "/" only.
        let closing = re.closing_loc();
        let closing_src = &ctx.source[closing.start_offset()..closing.end_offset()];
        if closing_src != "/" {
            return vec![];
        }

        let content_loc = re.content_loc();
        let content = &ctx.source[content_loc.start_offset()..content_loc.end_offset()];
        // RuboCop skips `/ /` (exactly one space).
        if content == " " {
            return vec![];
        }

        let full_loc = re.location();
        let regexp_src = &ctx.source[full_loc.start_offset()..full_loc.end_offset()];
        if !deterministic(regexp_src) {
            return vec![];
        }

        let replacement_arg = replacement_from_content(content);
        let prefer = preferred_argument(replacement_arg, self.enforce_double_quotes);
        let msg = format!(
            "Use string `{}` as argument instead of regexp `{}`.",
            prefer, regexp_src
        );

        let off_start = full_loc.start_offset();
        let off_end = full_loc.end_offset();
        vec![ctx
            .offense_with_range(self.name(), &msg, self.severity(), off_start, off_end)
            .with_correction(Correction::replace(off_start, off_end, prefer))]
    }
}

crate::register_cop!("Style/RedundantRegexpArgument", |cfg| {
    let enforce_double_quotes = cfg
        .get_cop_config("Style/StringLiterals")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| s == "double_quotes")
        .unwrap_or(false);
    Some(Box::new(RedundantRegexpArgument::with_config(enforce_double_quotes)))
});
