//! Style/QuotedSymbols cop
//!
//! Checks if the quotes used for quoted symbols match the configured defaults.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Style/QuotedSymbols";
const MSG_SINGLE: &str = "Prefer single-quoted symbols when you don't need string interpolation or special symbols.";
const MSG_DOUBLE: &str = "Prefer double-quoted symbols unless you need single quotes to avoid extra backslashes for escaping.";

#[derive(Debug, Clone, PartialEq)]
pub enum EffectiveStyle {
    SingleQuotes,
    DoubleQuotes,
}

pub struct QuotedSymbols {
    style: EffectiveStyle,
}

impl QuotedSymbols {
    pub fn new(style: EffectiveStyle) -> Self {
        Self { style }
    }
}

impl Cop for QuotedSymbols {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    cop: &'a QuotedSymbols,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode) {
        let start = node.location().start_offset();
        let prism_end = node.location().end_offset();
        let src = self.ctx.src(start, prism_end);

        // Distinguish forms:
        //   `:'a'` / `:"a"`            - standalone, len >=4, starts with `:`
        //   `'a':` / `"a":`            - colon-style hash key, ends with `:`
        //   `:a` / plain `a:` / `%i[..]` items - skip
        let (is_hash_colon, body_src, body_start, body_end): (bool, &str, usize, usize) =
            if src.starts_with(':') {
                if src.len() < 4 {
                    return;
                }
                let inner = &src[1..];
                if !(inner.starts_with('\'') || inner.starts_with('"')) {
                    return;
                }
                (false, inner, start + 1, prism_end)
            } else if src.ends_with(':') && src.len() >= 4 {
                // Colon-style hash key: SymbolNode location includes trailing `:`
                let inner = &src[..src.len() - 1];
                if !(inner.starts_with('\'') || inner.starts_with('"')) {
                    return;
                }
                (true, inner, start, prism_end - 1)
            } else {
                return;
            };

        let _ = is_hash_colon;

        // Multi-line symbols: RuboCop accepts these regardless of style
        if body_src.contains('\n') {
            return;
        }

        // body_src is the quoted form e.g. `'a'` or `"a"`
        let first = body_src.as_bytes()[0];
        let is_double = first == b'"';
        let is_single = first == b'\'';
        if !is_double && !is_single {
            return;
        }

        let flag = match self.cop.style {
            EffectiveStyle::SingleQuotes => {
                // Flag double-quoted if it doesn't require double quotes.
                if !is_double {
                    return;
                }
                // wrong_quotes? for single_quotes style: !double_quotes_required?(body_src)
                !double_quotes_required(body_src)
            }
            EffectiveStyle::DoubleQuotes => {
                // wrong_quotes? for double_quotes style: !/"|\\[^'\\]|#[@{$]/.match(body_src)
                let wrong = !double_quote_problematic(body_src);
                let invalid_dq = if is_double {
                    // invalid_double_quotes?: !/"|(?<!\\)\\[aAbcdefkMnprsStuUxzZ0-7]|#[@{$]/.match(node.source)
                    !invalid_double_quotes_pattern(src)
                } else {
                    false
                };
                wrong || invalid_dq
            }
        };

        if !flag {
            return;
        }

        // Use body offsets (excluding trailing `:` for hash-colon form) as offense range
        let msg = match self.cop.style {
            EffectiveStyle::SingleQuotes => MSG_SINGLE,
            EffectiveStyle::DoubleQuotes => MSG_DOUBLE,
        };
        // Offense covers node.source from `:` (or quote) to closing quote — mirrors RuboCop's add_offense(node)
        // For standalone: full source. For hash-colon: source minus trailing `:`.
        let off_start = start;
        let off_end = body_end;
        let target_double = matches!(self.cop.style, EffectiveStyle::DoubleQuotes);
        let inner = &body_src[1..body_src.len() - 1];
        let new_inner = reescape_quotes(inner, target_double);
        let new_quote = if target_double { '"' } else { '\'' };
        let replacement = format!("{}{}{}", new_quote, new_inner, new_quote);
        let mut offense = self.ctx.offense_with_range(
            COP_NAME,
            msg,
            Severity::Convention,
            off_start,
            off_end,
        );
        offense = offense.with_correction(Correction::replace(body_start, body_end, replacement));
        self.offenses.push(offense);
    }
}

/// Mirrors RuboCop Util.double_quotes_required?:
/// `/' | (?<! \\) \\{2}* \\ (?![\\"])/x`
/// True if string contains `'` or a backslash that's not paired/escaped.
fn double_quotes_required(s: &str) -> bool {
    if s.contains('\'') {
        return true;
    }
    // Find a backslash with even-count preceding backslashes, not followed by `\` or `"`.
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Count run of `\\` starting here.
            let mut run = 0;
            while i + run < bytes.len() && bytes[i + run] == b'\\' {
                run += 1;
            }
            // After run of backslashes, we have run \\ chars consumed.
            // The pattern wants a backslash NOT preceded by another \\ — i.e. the LAST one of an odd-length run,
            // not followed by `\` (always true at end of run) or `"`.
            if run % 2 == 1 {
                // Odd: last char of run is a "lonely" backslash
                let after = i + run;
                if after >= bytes.len() {
                    // End of string — followed by "nothing" which isn't `\\` or `"` → matches
                    return true;
                }
                let next = bytes[after];
                if next != b'\\' && next != b'"' {
                    return true;
                }
            }
            i += run;
        } else {
            i += 1;
        }
    }
    false
}

/// Pattern `/" | \\[^'\\] | \#[@{$]/x` — true if matches.
/// (Used in StringLiteralsHelp wrong_quotes? for double_quotes style.)
fn double_quote_problematic(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            return true;
        }
        if b == b'\\' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            if n != b'\'' && n != b'\\' {
                return true;
            }
            i += 2;
            continue;
        }
        if b == b'#' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            if n == b'@' || n == b'{' || n == b'$' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Pattern `/" | (?<!\\)\\[aAbcdefkMnprsStuUxzZ0-7] | \#[@{$]/x` — true if matches.
fn invalid_double_quotes_pattern(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    let chars: &[u8] = b"aAbcdefkMnprsStuUxzZ01234567";
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            return true;
        }
        if b == b'\\' {
            // Check (?<!\\) — preceding byte must NOT be `\`. We've consumed any preceding `\\` pairs.
            // Use parity of preceding consecutive `\` count: 0 = lookbehind passes.
            let mut prev_back = 0;
            let mut j = i;
            while j > 0 && bytes[j - 1] == b'\\' {
                prev_back += 1;
                j -= 1;
            }
            if prev_back == 0 && i + 1 < bytes.len() && chars.contains(&bytes[i + 1]) {
                return true;
            }
        }
        if b == b'#' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            if n == b'@' || n == b'{' || n == b'$' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Re-escape inner content when swapping quote style.
/// `target_double=true` means going to double-quotes (was single): unescape `\'` → `'`.
/// `target_double=false` means going to single-quotes (was double): unescape `\"` → `"`.
/// Leave other escape sequences (`\\`, `\n`, etc.) intact.
fn reescape_quotes(inner: &str, target_double: bool) -> String {
    let bytes = inner.as_bytes();
    let mut out = String::with_capacity(inner.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            if n == b'\\' {
                out.push('\\');
                out.push('\\');
                i += 2;
                continue;
            }
            if target_double && n == b'\'' {
                out.push('\'');
                i += 2;
                continue;
            }
            if !target_double && n == b'"' {
                out.push('"');
                i += 2;
                continue;
            }
            out.push('\\');
            out.push(n as char);
            i += 2;
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

crate::register_cop!("Style/QuotedSymbols", |cfg| {
    let cc = cfg.get_cop_config("Style/QuotedSymbols");
    let raw = cc
        .and_then(|c| c.enforced_style.clone())
        .unwrap_or_else(|| "same_as_string_literals".to_string());

    let style = match raw.as_str() {
        "single_quotes" => EffectiveStyle::SingleQuotes,
        "double_quotes" => EffectiveStyle::DoubleQuotes,
        _ => {
            // same_as_string_literals: defer to Style/StringLiterals
            if !cfg.is_cop_enabled("Style/StringLiterals") {
                EffectiveStyle::SingleQuotes
            } else {
                let sl = cfg
                    .get_cop_config("Style/StringLiterals")
                    .and_then(|c| c.enforced_style.clone());
                match sl.as_deref() {
                    Some("double_quotes") => EffectiveStyle::DoubleQuotes,
                    _ => EffectiveStyle::SingleQuotes,
                }
            }
        }
    };
    Some(Box::new(QuotedSymbols::new(style)))
});
