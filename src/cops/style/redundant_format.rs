//! Style/RedundantFormat - flag redundant `format`/`sprintf` calls.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_format.rb
//!
//! Two modes:
//!   1. Single-arg form: `format(str|dstr|const)` with no extra args → replace by arg source.
//!   2. Literal-args form: `format("literal", lit1, lit2, ...)` where all format sequences
//!      can be resolved from literal arguments → replace by the pre-computed string.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct RedundantFormat;

impl RedundantFormat {
    pub fn new() -> Self { Self }
}

// ─── Receiver detection ───────────────────────────────────────────────────

fn is_kernel_receiver(recv: &Node) -> bool {
    match recv {
        Node::ConstantReadNode { .. } => {
            let c = recv.as_constant_read_node().unwrap();
            let name = String::from_utf8_lossy(c.name().as_slice());
            name == "Kernel"
        }
        Node::ConstantPathNode { .. } => {
            let p = recv.as_constant_path_node().unwrap();
            // Must be root-rooted (::Kernel) with no parent namespace.
            if p.parent().is_some() { return false; }
            match p.name() {
                Some(n) => String::from_utf8_lossy(n.as_slice()) == "Kernel",
                None => false,
            }
        }
        _ => false,
    }
}

fn valid_receiver(recv: Option<Node>) -> bool {
    match recv {
        None => true,
        Some(n) => is_kernel_receiver(&n),
    }
}

// ─── Argument inspection ──────────────────────────────────────────────────

fn has_splat_or_dsplat(args: &[Node]) -> bool {
    args.iter().any(|a| {
        matches!(a, Node::SplatNode { .. } | Node::ForwardingArgumentsNode { .. })
        || is_hash_with_kwsplat(a)
    })
}

fn is_hash_with_kwsplat(n: &Node) -> bool {
    match n {
        Node::KeywordHashNode { .. } => {
            let h = n.as_keyword_hash_node().unwrap();
            h.elements().iter().any(|e| matches!(e, Node::AssocSplatNode { .. }))
        }
        Node::HashNode { .. } => {
            let h = n.as_hash_node().unwrap();
            h.elements().iter().any(|e| matches!(e, Node::AssocSplatNode { .. }))
        }
        _ => false,
    }
}

// Unwrap single-statement ParenthesesNode.
fn unwrap_parens<'a>(n: &'a Node<'a>) -> Option<Node<'a>> {
    let p = n.as_parentheses_node()?;
    let body = p.body()?;
    let stmts = body.as_statements_node()?;
    let list: Vec<Node> = stmts.body().iter().collect();
    if list.len() == 1 { Some(list.into_iter().next().unwrap()) } else { None }
}

// ─── Form 1: single-arg (str / dstr / constant) ──────────────────────────

fn form1_replacement_source<'a>(arg: &Node, src: &'a str) -> Option<String> {
    match arg {
        Node::StringNode { .. } => {
            let s = arg.as_string_node().unwrap();
            let loc = s.location();
            let raw = &src[loc.start_offset()..loc.end_offset()];
            // For plain string literals, escape control chars in the *decoded* bytes.
            // But the spec expects the original source preserved when possible, and
            // control chars only expanded when the source literally contains them.
            // We escape control bytes appearing in the raw source text.
            Some(escape_control_chars(raw))
        }
        Node::InterpolatedStringNode { .. } => {
            Some(node_source(arg, src).to_string())
        }
        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => {
            Some(node_source(arg, src).to_string())
        }
        _ => None,
    }
}

fn node_source<'a>(n: &Node, src: &'a str) -> &'a str {
    let loc = n.location();
    &src[loc.start_offset()..loc.end_offset()]
}

// Escape control chars: chars with codepoint < 0x20 or == 0x7f.
// Mirrors RuboCop's `string.dump[1..-2]` behaviour on individual bytes.
fn escape_control_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\t'   => out.push_str("\\t"),
            '\n'   => out.push_str("\\n"),
            '\x0b' => out.push_str("\\v"),
            '\x0c' => out.push_str("\\f"),
            '\r'   => out.push_str("\\r"),
            '\x1b' => out.push_str("\\e"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ─── Form 2: literal-args inlining ─────────────────────────────────────────

// Extract delimiters for a StringNode.
// Returns (open, close) strings from the source, or None if opening_loc is absent.
fn string_delimiters<'a>(s: &ruby_prism::StringNode, src: &'a str) -> Option<(&'a str, &'a str)> {
    let open = s.opening_loc()?;
    let close = s.closing_loc()?;
    Some((
        &src[open.start_offset()..open.end_offset()],
        &src[close.start_offset()..close.end_offset()],
    ))
}

// Get the raw content between the delimiters of a string literal (used to
// detect `#{...}` in a literal string that Prism already parsed as StringNode
// not InterpolatedStringNode — this shouldn't happen, but we scan conservatively).
fn string_content<'a>(s: &ruby_prism::StringNode, src: &'a str) -> &'a str {
    let open = s.opening_loc();
    let close = s.closing_loc();
    let loc = s.location();
    let start = open.map(|o| o.end_offset()).unwrap_or(loc.start_offset());
    let end = close.map(|c| c.start_offset()).unwrap_or(loc.end_offset());
    &src[start..end]
}

// Decoded content of a string (with escapes resolved). We use `unescaped()` bytes.
fn string_value(s: &ruby_prism::StringNode) -> String {
    String::from_utf8_lossy(s.unescaped().as_ref()).to_string()
}

// Format sequence parsed from the template.
#[derive(Debug, Clone)]
struct FormatSeq {
    source: String,              // exact substring matched
    begin_pos: usize,            // byte-offset into template content
    end_pos: usize,              // exclusive end
    flags: String,
    width: Option<String>,       // literal digits, "*", or "*N$"
    precision: Option<String>,   // "" means precision specified but empty; None means absent
    name: Option<String>,
    type_char: char,             // 's' 'd' 'i' 'u' 'f' 'b' 'B' ... or '%' for escaped
    arg_number: Option<usize>,   // N in %N$... (outer, not star)
    is_template: bool,           // %{name}
}

impl FormatSeq {
    fn is_percent(&self) -> bool { self.type_char == '%' }
    fn is_annotated(&self) -> bool { self.name.is_some() && !self.is_template }
    fn variable_width(&self) -> bool {
        self.width.as_deref().map_or(false, |w| w.starts_with('*'))
    }
    fn variable_precision(&self) -> bool {
        self.precision.as_deref().map_or(false, |p| p.starts_with('*'))
    }
    fn variable_width_arg_number(&self) -> Option<usize> {
        let w = self.width.as_deref()?;
        if !w.starts_with('*') { return None; }
        if w == "*" { return Some(1); }
        // "*N$"
        let num: String = w.chars().skip(1).take_while(|c| c.is_ascii_digit()).collect();
        num.parse().ok()
    }
}

// Parse format sequences in a template.
// Returns None if there's an interpolation-like `#{...}` in content (we operate on the
// raw source not the unescaped one, so callers pre-check dstr vs str).
fn parse_format_sequences(template: &str) -> Option<Vec<FormatSeq>> {
    let bytes = template.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        if bytes[i] != b'%' { i += 1; continue; }
        let start = i;
        i += 1;
        if i >= bytes.len() { return None; /* trailing % — invalid */ }
        if bytes[i] == b'%' {
            out.push(FormatSeq {
                source: "%%".into(), begin_pos: start, end_pos: i + 1,
                flags: String::new(), width: None, precision: None, name: None,
                type_char: '%', arg_number: None, is_template: false,
            });
            i += 1;
            continue;
        }

        // Possible outer arg number: digits followed by '$'
        let mut arg_number: Option<usize> = None;
        {
            let saved = i;
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_digit() { j += 1; }
            if j > saved && j < bytes.len() && bytes[j] == b'$' {
                let n: usize = std::str::from_utf8(&bytes[saved..j]).ok()?.parse().ok()?;
                arg_number = Some(n);
                i = j + 1;
            }
        }

        // Flags
        let mut flags = String::new();
        while i < bytes.len() && matches!(bytes[i], b' ' | b'#' | b'0' | b'+' | b'-') {
            flags.push(bytes[i] as char);
            i += 1;
        }

        // Template form: %{name}
        if i < bytes.len() && bytes[i] == b'{' {
            let name_start = i + 1;
            let mut j = name_start;
            while j < bytes.len() && bytes[j] != b'}' { j += 1; }
            if j >= bytes.len() { return None; }
            let name = std::str::from_utf8(&bytes[name_start..j]).ok()?.to_string();
            i = j + 1;
            out.push(FormatSeq {
                source: std::str::from_utf8(&bytes[start..i]).ok()?.to_string(),
                begin_pos: start, end_pos: i,
                flags, width: None, precision: None, name: Some(name),
                type_char: 's', // template types are implicit %s-ish (RuboCop treats the literal differently)
                arg_number, is_template: true,
            });
            continue;
        }

        // Width — either "*" (optionally "*N$"), digits, or empty.
        let mut width: Option<String> = None;
        if i < bytes.len() && bytes[i] == b'*' {
            let ws = i;
            i += 1;
            let digs_start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            if i > digs_start && i < bytes.len() && bytes[i] == b'$' {
                i += 1;
                width = Some(std::str::from_utf8(&bytes[ws..i]).ok()?.to_string());
            } else {
                width = Some("*".to_string());
                i = digs_start; // back up — keep the digits for nothing (no $)
                if digs_start != ws + 1 { i = digs_start; }
            }
        } else {
            let ws = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            if i > ws { width = Some(std::str::from_utf8(&bytes[ws..i]).ok()?.to_string()); }
        }

        // Precision
        let mut precision: Option<String> = None;
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'*' {
                let ps = i;
                i += 1;
                let digs_start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                if i > digs_start && i < bytes.len() && bytes[i] == b'$' {
                    i += 1;
                    precision = Some(std::str::from_utf8(&bytes[ps..i]).ok()?.to_string());
                } else {
                    precision = Some("*".to_string());
                }
            } else {
                let ps = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                precision = Some(std::str::from_utf8(&bytes[ps..i]).ok()?.to_string());
            }
        }

        // Annotated name: %<name>
        let mut name: Option<String> = None;
        if i < bytes.len() && bytes[i] == b'<' {
            let name_start = i + 1;
            let mut j = name_start;
            while j < bytes.len() && bytes[j] != b'>' { j += 1; }
            if j >= bytes.len() { return None; }
            name = Some(std::str::from_utf8(&bytes[name_start..j]).ok()?.to_string());
            i = j + 1;
        }

        // More flags allowed after name
        while i < bytes.len() && matches!(bytes[i], b' ' | b'#' | b'0' | b'+' | b'-') {
            flags.push(bytes[i] as char);
            i += 1;
        }

        // Width again (allowed to come after annotated name+flags).
        if width.is_none() {
            if i < bytes.len() && bytes[i] == b'*' {
                let ws = i;
                i += 1;
                let digs_start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                if i > digs_start && i < bytes.len() && bytes[i] == b'$' {
                    i += 1;
                    width = Some(std::str::from_utf8(&bytes[ws..i]).ok()?.to_string());
                } else {
                    width = Some("*".to_string());
                }
            } else {
                let ws = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                if i > ws { width = Some(std::str::from_utf8(&bytes[ws..i]).ok()?.to_string()); }
            }
        }
        if precision.is_none() {
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
                if i < bytes.len() && bytes[i] == b'*' {
                    let ps = i;
                    i += 1;
                    let digs_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                    if i > digs_start && i < bytes.len() && bytes[i] == b'$' {
                        i += 1;
                        precision = Some(std::str::from_utf8(&bytes[ps..i]).ok()?.to_string());
                    } else {
                        precision = Some("*".to_string());
                    }
                } else {
                    let ps = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                    precision = Some(std::str::from_utf8(&bytes[ps..i]).ok()?.to_string());
                }
            }
        }

        // Type char
        if i >= bytes.len() { return None; }
        let tc = bytes[i] as char;
        if !matches!(tc, 'b'|'B'|'d'|'i'|'o'|'u'|'x'|'X'|'e'|'E'|'f'|'g'|'G'|'a'|'A'|'c'|'p'|'s') {
            return None;
        }
        i += 1;
        out.push(FormatSeq {
            source: std::str::from_utf8(&bytes[start..i]).ok()?.to_string(),
            begin_pos: start, end_pos: i,
            flags, width, precision, name,
            type_char: tc, arg_number, is_template: false,
        });
    }
    Some(out)
}

// Quickly reject a template that contains `#{` (Prism produced StringNode only if
// no interpolation, but defensive).
fn has_interpolation_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'#' && b[i + 1] == b'{' { return true; }
        i += 1;
    }
    false
}

// ─── Literal argument value extraction ────────────────────────────────────

#[derive(Debug, Clone)]
enum LitVal {
    Str(String),   // decoded value
    Int(i64),
    Float(f64),
    Bool(bool),
    Nil,
    // For numeric-only use (Rational/Complex), store stringified value.
    Rational(f64),
    Complex(f64 /* real */, f64 /* imag */),
}

impl LitVal {
    // "Stringify" for %s — matches Ruby's behaviour on acceptable literals.
    fn to_s(&self) -> String {
        match self {
            LitVal::Str(s) => s.clone(),
            LitVal::Int(i) => i.to_string(),
            LitVal::Float(f) => {
                // Ruby 1.1.to_s == "1.1", 1.0.to_s == "1.0"
                ruby_float_to_s(*f)
            }
            LitVal::Bool(b) => b.to_string(),
            LitVal::Nil => String::new(),
            LitVal::Rational(f) => {
                // Ruby: (1/1r).to_s == "1/1", (3/8r).to_s == "3/8"
                // We only have the decimal value — fall back is caller-driven.
                // But %s on a Rational uses to_s which yields "num/den". We store
                // the numerator/denominator form as a string elsewhere.
                // Should not reach here for Rational under %s — caller stringifies directly.
                format!("{}", f)
            }
            LitVal::Complex(r, i) => format!("{}{:+}i", ruby_num_simple(*r), ruby_num_simple(*i)),
        }
    }
}

// Format a float the way Ruby does for %s on a Float: "5.5", "5.0", "0.375"
fn ruby_float_to_s(f: f64) -> String {
    if f.is_nan() { return "NaN".into(); }
    if f.is_infinite() { return if f > 0.0 { "Infinity".into() } else { "-Infinity".into() }; }
    if f == f.trunc() && f.abs() < 1e16 {
        return format!("{:.1}", f);
    }
    // Use shortest representation via default fmt.
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') { s } else { format!("{}.0", f) }
}

fn ruby_num_simple(f: f64) -> String {
    if f == f.trunc() && f.abs() < 1e16 {
        return format!("{}", f as i64);
    }
    format!("{}", f)
}

// A parsed literal arg carries both a Ruby-level value (for evaluating `format`) and
// a flag for dstr (which changes quoting decisions).
#[derive(Debug, Clone)]
struct LitArg {
    val: LitVal,
    // Original AST node source (used for annotated dstr output).
    source: String,
    // True if the argument is a dstr or dsym (interpolated string/sym).
    is_dstr: bool,
    // Original rational/complex representations as Ruby source, for precise to_s().
    rational_num_den: Option<(i128, i128)>,
    complex_real_imag: Option<(f64, f64)>,
}

fn parse_rational(node: &Node, src: &str) -> Option<(i128, i128)> {
    match node {
        Node::RationalNode { .. } => {
            // source like "1r", "8r" → numerator, denom=1
            let s = node_source(node, src);
            let trimmed = s.trim_end_matches('r');
            let n: i128 = trimmed.parse().ok()?;
            Some((n, 1))
        }
        Node::CallNode { .. } => {
            // int / rational
            let c = node.as_call_node().unwrap();
            if node_name!(c) != "/" { return None; }
            let recv = c.receiver()?;
            if !matches!(recv, Node::IntegerNode { .. }) { return None; }
            let arg = c.arguments()?.arguments().iter().next()?;
            if !matches!(arg, Node::RationalNode { .. }) { return None; }
            let num_s = node_source(&recv, src);
            let n: i128 = num_s.parse().ok()?;
            let den_s = node_source(&arg, src).trim_end_matches('r').to_string();
            let d: i128 = den_s.parse().ok()?;
            if d == 0 { return None; }
            Some((n, d))
        }
        _ => None,
    }
}

fn parse_complex(node: &Node, src: &str) -> Option<(f64, f64)> {
    match node {
        Node::ImaginaryNode { .. } => {
            // "1i" or "0i" → imag-only
            let s = node_source(node, src).trim_end_matches('i');
            let f: f64 = s.parse().ok()?;
            Some((0.0, f))
        }
        Node::CallNode { .. } => {
            // int '+' imaginary
            let c = node.as_call_node().unwrap();
            if node_name!(c) != "+" { return None; }
            let recv = c.receiver()?;
            if !matches!(recv, Node::IntegerNode { .. } | Node::FloatNode { .. }) { return None; }
            let arg = c.arguments()?.arguments().iter().next()?;
            if !matches!(arg, Node::ImaginaryNode { .. }) { return None; }
            let r: f64 = node_source(&recv, src).parse().ok()?;
            let im_s = node_source(&arg, src).trim_end_matches('i');
            let im: f64 = im_s.parse().ok()?;
            Some((r, im))
        }
        _ => None,
    }
}

fn extract_literal<'a>(node: &Node<'a>, src: &str) -> Option<LitArg> {
    // Peel through single-stmt ParenthesesNode.
    if let Some(inner) = unwrap_parens(node) {
        return extract_literal(&inner, src);
    }
    let source = node_source(node, src).to_string();
    match node {
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            Some(LitArg { val: LitVal::Str(string_value(&s)), source, is_dstr: false,
                          rational_num_den: None, complex_real_imag: None })
        }
        Node::InterpolatedStringNode { .. } => {
            // We don't know the runtime value — but RuboCop treats dstr as acceptable
            // for %s with no width/precision constraints. Use LitVal::Str with the
            // RAW source minus delimiters; output path needs special handling.
            Some(LitArg { val: LitVal::Str(String::new()), source, is_dstr: true,
                          rational_num_den: None, complex_real_imag: None })
        }
        Node::SymbolNode { .. } => {
            let s = node.as_symbol_node().unwrap();
            let bytes = s.unescaped();
            let v = String::from_utf8_lossy(bytes.as_ref()).to_string();
            Some(LitArg { val: LitVal::Str(v), source, is_dstr: false,
                          rational_num_den: None, complex_real_imag: None })
        }
        Node::InterpolatedSymbolNode { .. } => {
            // dsym — treat like dstr.
            Some(LitArg { val: LitVal::Str(String::new()), source, is_dstr: true,
                          rational_num_den: None, complex_real_imag: None })
        }
        Node::IntegerNode { .. } => {
            // Parse the integer value from its source slice (handles decimal/hex/oct/bin).
            let s = node_source(node, src).replace('_', "");
            let v = if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                i64::from_str_radix(rest, 16).ok()?
            } else if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                i64::from_str_radix(rest, 2).ok()?
            } else if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
                i64::from_str_radix(rest, 8).ok()?
            } else {
                s.parse::<i64>().ok()?
            };
            Some(LitArg { val: LitVal::Int(v), source, is_dstr: false,
                          rational_num_den: None, complex_real_imag: None })
        }
        Node::FloatNode { .. } => {
            let v: f64 = source.parse().ok()?;
            Some(LitArg { val: LitVal::Float(v), source, is_dstr: false,
                          rational_num_den: None, complex_real_imag: None })
        }
        Node::TrueNode { .. } => Some(LitArg {
            val: LitVal::Bool(true), source, is_dstr: false,
            rational_num_den: None, complex_real_imag: None }),
        Node::FalseNode { .. } => Some(LitArg {
            val: LitVal::Bool(false), source, is_dstr: false,
            rational_num_den: None, complex_real_imag: None }),
        Node::NilNode { .. } => Some(LitArg {
            val: LitVal::Nil, source, is_dstr: false,
            rational_num_den: None, complex_real_imag: None }),
        Node::RationalNode { .. } | Node::ImaginaryNode { .. } | Node::CallNode { .. } => {
            if let Some((n, d)) = parse_rational(node, src) {
                let f = n as f64 / d as f64;
                return Some(LitArg {
                    val: LitVal::Rational(f), source, is_dstr: false,
                    rational_num_den: Some((n, d)), complex_real_imag: None });
            }
            if let Some((r, im)) = parse_complex(node, src) {
                return Some(LitArg {
                    val: LitVal::Complex(r, im), source, is_dstr: false,
                    rational_num_den: None, complex_real_imag: Some((r, im)) });
            }
            None
        }
        _ => None,
    }
}

fn is_acceptable_for_s(v: &LitVal) -> bool {
    matches!(v,
        LitVal::Str(_) | LitVal::Int(_) | LitVal::Float(_)
        | LitVal::Bool(_) | LitVal::Nil
        | LitVal::Rational(_) | LitVal::Complex(_, _))
}

// For %d/%i/%u — integer-ish
fn as_integer(arg: &LitArg) -> Option<i64> {
    match &arg.val {
        LitVal::Int(i) => Some(*i),
        LitVal::Float(f) => Some(f.trunc() as i64),
        LitVal::Rational(f) => Some(f.trunc() as i64),
        LitVal::Complex(r, _) => Some(r.trunc() as i64),
        LitVal::Str(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn as_float(arg: &LitArg) -> Option<f64> {
    match &arg.val {
        LitVal::Int(i) => Some(*i as f64),
        LitVal::Float(f) => Some(*f),
        LitVal::Rational(f) => Some(*f),
        LitVal::Complex(r, _) => Some(*r),
        LitVal::Str(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

// ─── sprintf subset implementation ────────────────────────────────────────

struct ArgCursor<'a> {
    args: &'a [LitArg],
    positional: usize,
}

impl<'a> ArgCursor<'a> {
    fn new(args: &'a [LitArg]) -> Self { Self { args, positional: 0 } }
    fn next(&mut self) -> Option<&'a LitArg> {
        let a = self.args.get(self.positional)?;
        self.positional += 1;
        Some(a)
    }
    fn at(&self, i: usize /* 1-based */) -> Option<&'a LitArg> {
        if i == 0 { return None; }
        self.args.get(i - 1)
    }
}

fn resolve_star_value(
    val: &str,
    cursor: &mut ArgCursor,
) -> Option<i64> {
    if val == "*" {
        let a = cursor.next()?;
        match &a.val {
            LitVal::Int(i) => Some(*i),
            LitVal::Float(f) => Some(*f as i64),
            LitVal::Str(s) => s.parse().ok(),
            _ => None,
        }
    } else if let Some(rest) = val.strip_prefix('*') {
        let num: usize = rest.trim_end_matches('$').parse().ok()?;
        let a = cursor.at(num)?;
        match &a.val {
            LitVal::Int(i) => Some(*i),
            LitVal::Float(f) => Some(*f as i64),
            LitVal::Str(s) => s.parse().ok(),
            _ => None,
        }
    } else {
        val.parse().ok()
    }
}

// Stringify an arg for %s. For acceptable literals, match Ruby's to_s.
fn stringify_for_s(arg: &LitArg) -> Option<String> {
    match &arg.val {
        LitVal::Str(s) => Some(s.clone()),
        LitVal::Int(i) => Some(i.to_string()),
        LitVal::Float(f) => Some(ruby_float_to_s(*f)),
        // RuboCop uses the argument source for these (via argument_value fallthrough
        // since TrueNode/FalseNode/NilNode lack `.value`).
        LitVal::Bool(_) | LitVal::Nil => Some(arg.source.clone()),
        LitVal::Rational(_) => {
            let (n, d) = arg.rational_num_den?;
            Some(format!("{}/{}", n, d))
        }
        LitVal::Complex(r, i) => {
            Some(format!("{}{}{}i",
                ruby_num_simple(*r),
                if *i >= 0.0 { "+" } else { "-" },
                ruby_num_simple(i.abs())))
        }
    }
}

// Render an integer value with flags/width/precision.
fn render_integer(v: i64, flags: &str, width: Option<i64>, precision: Option<Option<i64>>) -> String {
    // precision: Some(None) == ".", Some(Some(n)) == ".n", None == absent
    let abs = v.unsigned_abs();
    let sign_str = if v < 0 {
        "-"
    } else if flags.contains('+') {
        "+"
    } else if flags.contains(' ') {
        " "
    } else {
        ""
    };

    // Digits part with precision
    let digits = match precision {
        Some(Some(p)) => {
            if p == 0 && v == 0 {
                String::new()
            } else {
                format!("{:0>width$}", abs, width = p as usize)
            }
        }
        Some(None) => {
            // bare `.` — Ruby treats equivalent to .0
            if v == 0 { String::new() } else { abs.to_string() }
        }
        None => abs.to_string(),
    };

    let body = format!("{}{}", sign_str, digits);
    if let Some(w) = width {
        let w = w as usize;
        if body.len() >= w { return body; }
        let pad = w - body.len();
        if flags.contains('-') {
            format!("{}{}", body, " ".repeat(pad))
        } else if flags.contains('0') && !precision.is_some() {
            // zero-pad AFTER sign
            format!("{}{}{}", sign_str, "0".repeat(pad), digits)
        } else {
            format!("{}{}", " ".repeat(pad), body)
        }
    } else {
        body
    }
}

fn render_float(v: f64, flags: &str, width: Option<i64>, precision: Option<Option<i64>>) -> String {
    let prec = match precision {
        Some(Some(p)) => p as usize,
        Some(None) => 0,
        None => 6,
    };
    let sign_str = if v.is_sign_negative() && !(v == 0.0 && !v.is_sign_negative()) && v < 0.0 {
        "-"
    } else if flags.contains('+') {
        "+"
    } else if flags.contains(' ') {
        " "
    } else {
        ""
    };
    let body_num = format!("{:.*}", prec, v.abs());
    let body = format!("{}{}", sign_str, body_num);
    if let Some(w) = width {
        let w = w as usize;
        if body.len() >= w { return body; }
        let pad = w - body.len();
        if flags.contains('-') {
            format!("{}{}", body, " ".repeat(pad))
        } else if flags.contains('0') {
            format!("{}{}{}", sign_str, "0".repeat(pad), body_num)
        } else {
            format!("{}{}", " ".repeat(pad), body)
        }
    } else {
        body
    }
}

fn render_string(s: &str, flags: &str, width: Option<i64>, precision: Option<Option<i64>>) -> String {
    let truncated: String = match precision {
        Some(Some(p)) => s.chars().take(p as usize).collect(),
        Some(None) => String::new(),
        None => s.to_string(),
    };
    if let Some(w) = width {
        let w = w as usize;
        if truncated.chars().count() >= w { return truncated; }
        let pad = w - truncated.chars().count();
        if flags.contains('-') {
            format!("{}{}", truncated, " ".repeat(pad))
        } else {
            format!("{}{}", " ".repeat(pad), truncated)
        }
    } else {
        truncated
    }
}

// Execute the format. Returns the produced string; None on any mismatch.
fn execute_format(
    template: &str,
    seqs: &[FormatSeq],
    args: &[LitArg],
    hash: Option<&[(String, LitArg)]>,
) -> Option<String> {
    let mut out = String::new();
    let mut last = 0usize;
    let mut cursor = ArgCursor::new(args);

    for seq in seqs {
        if seq.begin_pos > last {
            out.push_str(&template[last..seq.begin_pos]);
        }
        last = seq.end_pos;

        if seq.is_percent() { out.push('%'); continue; }

        // Resolve width
        let width_val: Option<i64> = if let Some(w) = &seq.width {
            Some(resolve_star_value(w, &mut cursor)?)
        } else { None };
        // Resolve precision: keep distinction between None (absent) and Some(Some(n)) / Some(None).
        let precision_val: Option<Option<i64>> = match &seq.precision {
            None => None,
            Some(p) if p.is_empty() => Some(None),
            Some(p) => Some(Some(resolve_star_value(p, &mut cursor)?)),
        };

        // Pick the arg
        let arg_ref: Option<&LitArg>;
        let arg_owned: Option<LitArg>;
        if seq.is_template {
            let name = seq.name.as_ref()?;
            let pairs = hash?;
            let found = pairs.iter().find(|(k, _)| k == name)?;
            arg_owned = Some(found.1.clone());
            arg_ref = None;
        } else if let Some(name) = &seq.name {
            // Annotated → hash lookup
            let pairs = hash?;
            let found = pairs.iter().find(|(k, _)| k == name)?;
            arg_owned = Some(found.1.clone());
            arg_ref = None;
        } else if let Some(n) = seq.arg_number {
            arg_ref = cursor.at(n);
            arg_owned = None;
            if arg_ref.is_none() { return None; }
        } else {
            arg_ref = cursor.next();
            arg_owned = None;
            if arg_ref.is_none() { return None; }
        }
        let arg = arg_ref.unwrap_or_else(|| arg_owned.as_ref().unwrap());

        match seq.type_char {
            's' => {
                // For %{name} template — literal always stringified like %s.
                let s = stringify_for_s(arg)?;
                // Template: width/precision don't apply (the template style %{name}
                // treats the text after } as literal). But annotated %<n>s does apply
                // width/precision. RuboCop excludes `.width || .precision` with dstr.
                if seq.is_template {
                    out.push_str(&s);
                } else {
                    out.push_str(&render_string(&s, &seq.flags, width_val, precision_val));
                }
            }
            'd' | 'i' | 'u' => {
                let v = as_integer(arg)?;
                out.push_str(&render_integer(v, &seq.flags, width_val, precision_val));
            }
            'f' => {
                let v = as_float(arg)?;
                out.push_str(&render_float(v, &seq.flags, width_val, precision_val));
            }
            _ => return None,
        }
    }
    if last < template.len() { out.push_str(&template[last..]); }
    Some(out)
}

// Determine whether all format sequences can be safely resolved.
fn all_fields_resolvable(
    seqs: &[FormatSeq],
    args: &[LitArg],
    hash: Option<&[(String, LitArg)]>,
) -> bool {
    if seqs.is_empty() { return false; }
    if seqs.iter().all(|s| s.is_percent()) { return false; }

    // Walk through sequences tracking cursor state; ensure each non-percent seq
    // maps to an acceptable literal.
    let mut positional = 0usize;
    for seq in seqs {
        if seq.is_percent() { continue; }

        // Variable width via *: require the width arg to be numeric literal.
        if seq.variable_width() {
            let wnum = match seq.variable_width_arg_number() {
                Some(n) => n, None => return false,
            };
            // If width is "*" (no $), it consumes a positional arg.
            if seq.width.as_deref() == Some("*") {
                let idx = positional;
                let a = match args.get(idx) { Some(a) => a, None => return false };
                if !matches!(a.val, LitVal::Int(_) | LitVal::Float(_) | LitVal::Str(_)) { return false; }
                if matches!(a.val, LitVal::Str(_)) {
                    // needs to parse as number
                    if as_integer(a).is_none() { return false; }
                }
                positional += 1;
            } else {
                let a = match args.get(wnum - 1) { Some(a) => a, None => return false };
                if as_integer(a).is_none() { return false; }
            }
        }
        // Variable precision via .*
        if seq.variable_precision() {
            if seq.precision.as_deref() == Some("*") {
                let idx = positional;
                let a = match args.get(idx) { Some(a) => a, None => return false };
                if as_integer(a).is_none() { return false; }
                positional += 1;
            } else if let Some(p) = &seq.precision {
                if let Some(rest) = p.strip_prefix('*') {
                    let num: usize = match rest.trim_end_matches('$').parse() {
                        Ok(n) => n, Err(_) => return false,
                    };
                    let a = match args.get(num - 1) { Some(a) => a, None => return false };
                    if as_integer(a).is_none() { return false; }
                }
            }
        }

        // Now the value arg
        let arg_opt: Option<&LitArg> = if seq.is_template {
            let name = match &seq.name { Some(n) => n, None => return false };
            let pairs = match hash { Some(p) => p, None => return false };
            pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
        } else if let Some(name) = &seq.name {
            let pairs = match hash { Some(p) => p, None => return false };
            pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
        } else if let Some(n) = seq.arg_number {
            args.get(n - 1)
        } else {
            let a = args.get(positional);
            positional += 1;
            a
        };
        let arg = match arg_opt { Some(a) => a, None => return false };

        // dstr in value disallowed if width or precision set (RuboCop skips).
        let width_set = seq.width.is_some();
        let prec_set = seq.precision.is_some();
        if arg.is_dstr && (width_set || prec_set) && !seq.is_template {
            return false;
        }

        // Template seqs accept any literal type (RuboCop: any acceptable literal).
        if seq.is_template {
            if !is_acceptable_for_s(&arg.val) && !arg.is_dstr { return false; }
            continue;
        }

        match seq.type_char {
            's' => {
                if !is_acceptable_for_s(&arg.val) && !arg.is_dstr { return false; }
            }
            'd' | 'i' | 'u' => {
                if !matches!(arg.val, LitVal::Int(_)|LitVal::Float(_)|LitVal::Str(_)|LitVal::Rational(_)|LitVal::Complex(_, _)) { return false; }
                if as_integer(arg).is_none() { return false; }
            }
            'f' => {
                if !matches!(arg.val, LitVal::Int(_)|LitVal::Float(_)|LitVal::Str(_)|LitVal::Rational(_)|LitVal::Complex(_, _)) { return false; }
                if as_float(arg).is_none() { return false; }
            }
            _ => return false,
        }
    }
    true
}

// ─── Build replacement string with proper delimiters ──────────────────────

// Returns (open_delim, close_delim, any_dstr_in_args)
fn quote_for_replacement(
    template_node: &ruby_prism::StringNode,
    src: &str,
    any_dstr: bool,
) -> (String, String) {
    let (open, close) = match string_delimiters(template_node, src) {
        Some(d) => d,
        None => return ("\"".into(), "\"".into()),
    };
    if !any_dstr { return (open.to_string(), close.to_string()); }

    // Needs interpolation-capable delimiters.
    if open == "'" {
        return ("\"".to_string(), "\"".to_string());
    }
    // %q{ → %Q{
    if let Some(rest) = open.strip_prefix("%q") {
        let new_open = format!("%Q{}", rest);
        return (new_open, close.to_string());
    }
    (open.to_string(), close.to_string())
}

// ─── Main check ───────────────────────────────────────────────────────────

impl Cop for RedundantFormat {
    fn name(&self) -> &'static str { "Style/RedundantFormat" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let m: &str = &method;
        if m != "format" && m != "sprintf" { return vec![]; }

        if !valid_receiver(node.receiver()) { return vec![]; }

        let args_node = match node.arguments() { Some(a) => a, None => return vec![] };
        let all_args: Vec<Node> = args_node.arguments().iter().collect();
        if all_args.is_empty() { return vec![]; }

        // Reject if any splat/dsplat appears anywhere.
        if has_splat_or_dsplat(&all_args) { return vec![]; }

        // Form 1: single arg that's str / dstr / const.
        if all_args.len() == 1 {
            let a = &all_args[0];
            if let Some(repl) = form1_replacement_source(a, ctx.source) {
                return vec![build_offense(self, node, ctx, &repl, m)];
            }
            return vec![];
        }

        // Form 2: first arg must be a plain StringNode (not dstr/const).
        let first = &all_args[0];
        let tmpl_node = match first.as_string_node() { Some(s) => s, None => return vec![] };
        let rest_args = &all_args[1..];

        // Extract literals for each remaining arg.
        let mut lit_args: Vec<LitArg> = Vec::with_capacity(rest_args.len());
        let mut hash_pairs: Option<Vec<(String, LitArg)>> = None;
        for (idx, a) in rest_args.iter().enumerate() {
            match a {
                Node::HashNode { .. } | Node::KeywordHashNode { .. } => {
                    // Must be the LAST argument (Ruby kwargs).
                    if idx != rest_args.len() - 1 { return vec![]; }
                    let pairs = match extract_hash_pairs(a, ctx.source) {
                        Some(p) => p, None => return vec![],
                    };
                    hash_pairs = Some(pairs);
                }
                _ => {
                    let lit = match extract_literal(a, ctx.source) {
                        Some(l) => l, None => return vec![],
                    };
                    lit_args.push(lit);
                }
            }
        }

        // Parse the format string (from decoded unescaped content).
        // Use the decoded string value for parsing AND for building the output.
        let tmpl_value = string_value(&tmpl_node);

        // If there's an interpolation in the raw source (can't happen — Prism
        // would have made this a dstr), bail.
        let raw_content = string_content(&tmpl_node, ctx.source);
        if has_interpolation_literal(raw_content) { return vec![]; }

        let seqs = match parse_format_sequences(&tmpl_value) { Some(s) => s, None => return vec![] };
        if seqs.is_empty() || seqs.iter().all(|s| s.is_percent()) { return vec![]; }

        // If we got a hash but no sequence consumes it (or vice-versa), invalid.
        let has_named = seqs.iter().any(|s| s.name.is_some());
        if hash_pairs.is_some() && !has_named { return vec![]; }
        if has_named && hash_pairs.is_none() {
            // Some templates (%{...}) need hash args; missing hash → not autocorrectable.
            return vec![];
        }

        if !all_fields_resolvable(&seqs, &lit_args, hash_pairs.as_deref()) { return vec![]; }

        // Execute format.
        let result_str = match execute_format(&tmpl_value, &seqs, &lit_args, hash_pairs.as_deref()) {
            Some(r) => r, None => return vec![],
        };

        // Determine whether any arg referenced via these seqs is a dstr (requires
        // interpolation-capable delimiters).
        let any_dstr = arg_is_dstr_referenced(&seqs, &lit_args, hash_pairs.as_deref());

        // Build the replacement.
        let (open, close) = quote_for_replacement(&tmpl_node, ctx.source, any_dstr);
        let escaped = escape_control_chars(&result_str);
        // If any referenced arg is dstr, we need to inline the dstr source (like Ruby does).
        // But RuboCop's quote() uses escape_control_chars on the FORMATTED result, which
        // for dstr args literally includes `#{...}` — because Ruby's format with a dstr as
        // argument isn't actually evaluated. RuboCop cheats: it takes the dstr node's
        // source (minus its delimiters) and substitutes it.
        //
        // Our execute_format produces "" for dstr args (LitVal::Str("")). Instead, we
        // need a second pass that substitutes dstr-arg contents into the result.

        let final_content = substitute_dstr_into_result(
            &tmpl_value, &seqs, &lit_args, hash_pairs.as_deref(),
        ).unwrap_or(escaped);

        let replacement = format!("{}{}{}", open, final_content, close);
        vec![build_offense(self, node, ctx, &replacement, m)]
    }
}

fn arg_is_dstr_referenced(
    seqs: &[FormatSeq],
    args: &[LitArg],
    hash: Option<&[(String, LitArg)]>,
) -> bool {
    let mut positional = 0usize;
    for seq in seqs {
        if seq.is_percent() { continue; }
        // Skip over width/precision star consumptions
        if seq.width.as_deref() == Some("*") { positional += 1; }
        if seq.precision.as_deref() == Some("*") { positional += 1; }

        if seq.is_template || seq.name.is_some() {
            let name = match &seq.name { Some(n) => n, None => continue };
            if let Some(pairs) = hash {
                if let Some((_, v)) = pairs.iter().find(|(k, _)| k == name) {
                    if v.is_dstr { return true; }
                }
            }
        } else if let Some(n) = seq.arg_number {
            if let Some(a) = args.get(n - 1) {
                if a.is_dstr { return true; }
            }
        } else {
            if let Some(a) = args.get(positional) {
                if a.is_dstr { return true; }
            }
            positional += 1;
        }
    }
    false
}

// When dstr args are referenced, rebuild the content by substituting the dstr
// source (interior content, minus delimiters) in place of each `%s`.
// Only supports simple %s replacement with dstr; RuboCop limits width/precision on dstr.
fn substitute_dstr_into_result(
    template: &str,
    seqs: &[FormatSeq],
    args: &[LitArg],
    hash: Option<&[(String, LitArg)]>,
) -> Option<String> {
    let mut out = String::new();
    let mut last = 0usize;
    let mut positional = 0usize;

    for seq in seqs {
        if seq.begin_pos > last { out.push_str(&template[last..seq.begin_pos]); }
        last = seq.end_pos;
        if seq.is_percent() { out.push('%'); continue; }

        // Handle * width/precision consumption
        let mut width_val: Option<i64> = None;
        if let Some(w) = &seq.width {
            if w == "*" {
                let a = args.get(positional)?;
                width_val = Some(match &a.val {
                    LitVal::Int(i) => *i,
                    LitVal::Float(f) => *f as i64,
                    LitVal::Str(s) => s.parse().ok()?,
                    _ => return None,
                });
                positional += 1;
            } else if let Some(rest) = w.strip_prefix('*') {
                let num: usize = rest.trim_end_matches('$').parse().ok()?;
                let a = args.get(num - 1)?;
                width_val = Some(match &a.val {
                    LitVal::Int(i) => *i,
                    LitVal::Float(f) => *f as i64,
                    LitVal::Str(s) => s.parse().ok()?,
                    _ => return None,
                });
            } else {
                width_val = Some(w.parse().ok()?);
            }
        }
        let mut precision_val: Option<Option<i64>> = None;
        if let Some(p) = &seq.precision {
            if p.is_empty() {
                precision_val = Some(None);
            } else if p == "*" {
                let a = args.get(positional)?;
                let v: i64 = match &a.val {
                    LitVal::Int(i) => *i,
                    LitVal::Float(f) => *f as i64,
                    LitVal::Str(s) => s.parse().ok()?,
                    _ => return None,
                };
                precision_val = Some(Some(v));
                positional += 1;
            } else if let Some(rest) = p.strip_prefix('*') {
                let num: usize = rest.trim_end_matches('$').parse().ok()?;
                let a = args.get(num - 1)?;
                let v: i64 = match &a.val {
                    LitVal::Int(i) => *i,
                    LitVal::Float(f) => *f as i64,
                    LitVal::Str(s) => s.parse().ok()?,
                    _ => return None,
                };
                precision_val = Some(Some(v));
            } else {
                precision_val = Some(Some(p.parse().ok()?));
            }
        }

        // Look up arg.
        let arg: &LitArg = if seq.is_template || seq.name.is_some() {
            let name = seq.name.as_ref()?;
            let pairs = hash?;
            &pairs.iter().find(|(k, _)| k == name)?.1
        } else if let Some(n) = seq.arg_number {
            args.get(n - 1)?
        } else {
            let a = args.get(positional)?;
            positional += 1;
            a
        };

        if arg.is_dstr {
            // Strip delimiters from the arg source — for :"#{foo}" yield #{foo}, for "#{foo}" yield #{foo}.
            let content = strip_string_like_delims(&arg.source);
            out.push_str(&content);
        } else {
            // Fall back to normal formatting, then escape control chars.
            match seq.type_char {
                's' => {
                    let s = stringify_for_s(arg)?;
                    let rendered = if seq.is_template { s } else { render_string(&s, &seq.flags, width_val, precision_val) };
                    out.push_str(&escape_control_chars(&rendered));
                }
                'd' | 'i' | 'u' => {
                    let v = as_integer(arg)?;
                    out.push_str(&escape_control_chars(&render_integer(v, &seq.flags, width_val, precision_val)));
                }
                'f' => {
                    let v = as_float(arg)?;
                    out.push_str(&escape_control_chars(&render_float(v, &seq.flags, width_val, precision_val)));
                }
                _ => return None,
            }
        }
    }
    if last < template.len() { out.push_str(&escape_control_chars(&template[last..])); }
    Some(out)
}

fn strip_string_like_delims(src: &str) -> String {
    // Handles: 'x', "x", :x, :"x", :'x', %q{x}, %Q{x}, %{x}, %q[x], %Q(x), etc.
    // We find opening delimiter and matching closing delimiter by pattern.
    let s = src;
    // Symbol prefix
    let s = s.strip_prefix(':').unwrap_or(s);
    // %q / %Q / % forms
    if let Some(rest) = s.strip_prefix("%q").or_else(|| s.strip_prefix("%Q")).or_else(|| s.strip_prefix("%")) {
        // rest starts with delimiter char
        if let Some(first) = rest.chars().next() {
            let close = match first { '{' => '}', '[' => ']', '(' => ')', '<' => '>', c => c };
            let rest = &rest[first.len_utf8()..];
            return rest.strip_suffix(close).unwrap_or(rest).to_string();
        }
    }
    if let Some(rest) = s.strip_prefix('"') {
        return rest.strip_suffix('"').unwrap_or(rest).to_string();
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return rest.strip_suffix('\'').unwrap_or(rest).to_string();
    }
    s.to_string()
}

// Extract key/value pairs from a hash or keyword_hash node.
fn extract_hash_pairs(node: &Node, src: &str) -> Option<Vec<(String, LitArg)>> {
    let elements: Vec<Node> = match node {
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().collect(),
        Node::KeywordHashNode { .. } => node.as_keyword_hash_node().unwrap().elements().iter().collect(),
        _ => return None,
    };
    let mut pairs = Vec::with_capacity(elements.len());
    for el in elements {
        let pair = el.as_assoc_node()?;
        let key = pair.key();
        let key_str: String = match &key {
            Node::SymbolNode { .. } => {
                let s = key.as_symbol_node().unwrap();
                String::from_utf8_lossy(s.unescaped().as_ref()).to_string()
            }
            _ => return None,
        };
        let val = pair.value();
        let lit = extract_literal(&val, src)?;
        pairs.push((key_str, lit));
    }
    Some(pairs)
}

fn build_offense(
    cop: &dyn Cop,
    node: &ruby_prism::CallNode,
    ctx: &CheckContext,
    prefer: &str,
    method_name: &str,
) -> Offense {
    let loc = node.location();
    let msg = format!(
        "Use `{}` directly instead of `{}`.",
        prefer, method_name
    );
    ctx.offense_with_range(
        cop.name(), &msg, cop.severity(),
        loc.start_offset(), loc.end_offset(),
    ).with_correction(Correction::replace(
        loc.start_offset(), loc.end_offset(), prefer.to_string(),
    ))
}

crate::register_cop!("Style/RedundantFormat", |_cfg| Some(Box::new(RedundantFormat::new())));
