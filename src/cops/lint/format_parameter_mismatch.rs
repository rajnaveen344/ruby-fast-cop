//! Lint/FormatParameterMismatch - Detects mismatches between format string placeholders and arguments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG_INVALID: &str =
    "Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.";

#[derive(Default)]
pub struct FormatParameterMismatch;

impl FormatParameterMismatch {
    pub fn new() -> Self { Self }
}

impl Cop for FormatParameterMismatch {
    fn name(&self) -> &'static str { "Lint/FormatParameterMismatch" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        match method_name.as_str() {
            "format" | "sprintf" => self.check_format_sprintf(node, ctx, &method_name),
            "%" => self.check_percent(node, ctx),
            _ => vec![],
        }
    }
}

impl FormatParameterMismatch {
    fn check_format_sprintf(
        &self,
        node: &ruby_prism::CallNode,
        ctx: &CheckContext,
        method_name: &str,
    ) -> Vec<Offense> {
        // format/sprintf must be called without a receiver, or on Kernel
        if let Some(recv) = node.receiver() {
            if !is_kernel_const(&recv) {
                return vec![];
            }
        }

        let args: Vec<_> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => return vec![],
        };
        if args.len() < 2 {
            return vec![];
        }

        // First argument must be a string literal
        let first_arg = &args[0];
        if !is_string_type(first_arg) {
            return vec![];
        }

        let format_str = source_of(first_arg, ctx);

        // Check if it's a heredoc (starts with <<)
        if format_str.starts_with("<<") {
            return vec![];
        }

        // Parse the format string
        let sequences = match parse_format_sequences(&format_str) {
            Ok(seqs) => seqs,
            Err(_) => return vec![],
        };

        // Check for mixed format types
        if has_mixed_format_types(&sequences) {
            let (start, end) = method_loc(node);
            return vec![ctx.offense_with_range(self.name(), MSG_INVALID, self.severity(), start, end)];
        }

        // Check for splat arguments (skip if any non-first arg is a splat)
        if args[1..].iter().any(|a| matches!(a, Node::SplatNode { .. })) {
            return vec![];
        }

        let num_args = args.len() - 1; // exclude format string
        let expected = expected_fields_count(&sequences);

        if expected == 0 && is_dstr_or_array(first_arg) {
            return vec![];
        }

        if !matched_arguments_count(expected, num_args) {
            return vec![];
        }

        let (start, end) = method_loc(node);
        let message = format!(
            "Number of arguments ({}) to `{}` doesn't match the number of fields ({}).",
            num_args, method_name, expected
        );
        vec![ctx.offense_with_range(self.name(), &message, self.severity(), start, end)]
    }

    fn check_percent(
        &self,
        node: &ruby_prism::CallNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let args: Vec<_> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => return vec![],
        };
        if args.is_empty() {
            return vec![];
        }

        let first_arg = &args[0];

        // String#% with receiver being a string literal
        if is_string_type(&receiver) {
            let format_str = source_of(&receiver, ctx);

            // Heredoc check
            if format_str.starts_with("<<") {
                return vec![];
            }

            // Check if the argument is an array literal
            let is_array_arg = matches!(first_arg, Node::ArrayNode { .. });

            if !is_array_arg {
                // Single non-array argument: skip (could be hash, variable, etc.)
                return vec![];
            }

            let array_node = first_arg.as_array_node().unwrap();
            let array_elements: Vec<_> = array_node.elements().iter().collect();

            // Check for splat in array elements
            // For %, splats in the array are counted as elements (since it's an array literal)
            // But if splat is present alongside more than expected elements, we count non-splat elements + splat count
            let mut has_splat = false;
            let mut non_splat_count = 0;
            for elem in &array_elements {
                if matches!(elem, Node::SplatNode { .. }) {
                    has_splat = true;
                    non_splat_count += 1; // count splat as 1 for purposes of "more than expected"
                } else {
                    non_splat_count += 1;
                }
            }

            let sequences = match parse_format_sequences(&format_str) {
                Ok(seqs) => seqs,
                Err(_) => return vec![],
            };

            if has_mixed_format_types(&sequences) {
                let (start, end) = method_loc(node);
                return vec![ctx.offense_with_range(self.name(), MSG_INVALID, self.severity(), start, end)];
            }

            let expected = expected_fields_count(&sequences);

            if expected == 0 && array_elements.is_empty() {
                return vec![];
            }

            if expected == 0 && is_dstr_type(&receiver) {
                return vec![];
            }

            // For % with splat in array: only flag if non-splat count already exceeds expected
            if has_splat {
                // RuboCop flags % with array containing splat if total count (counting splat as 1) > expected
                if non_splat_count as i64 <= expected as i64 {
                    return vec![];
                }
            }

            let num_args = array_elements.len();
            if !matched_arguments_count(expected, num_args) {
                return vec![];
            }

            let (start, end) = method_loc(node);
            let message = format!(
                "Number of arguments ({}) to `String#%` doesn't match the number of fields ({}).",
                num_args, expected
            );
            return vec![ctx.offense_with_range(self.name(), &message, self.severity(), start, end)];
        }

        vec![]
    }
}

/// Get method location (the selector), falling back to the message location
fn method_loc(node: &ruby_prism::CallNode) -> (usize, usize) {
    if let Some(msg_loc) = node.message_loc() {
        (msg_loc.start_offset(), msg_loc.end_offset())
    } else {
        let loc = node.location();
        (loc.start_offset(), loc.end_offset())
    }
}

fn is_kernel_const(node: &Node) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(c.name().as_slice());
        return name.as_ref() == "Kernel";
    }
    false
}

fn is_string_type(node: &Node) -> bool {
    matches!(
        node,
        Node::StringNode { .. } | Node::InterpolatedStringNode { .. }
    )
}

fn is_dstr_type(node: &Node) -> bool {
    matches!(node, Node::InterpolatedStringNode { .. })
}

fn is_dstr_or_array(node: &Node) -> bool {
    matches!(
        node,
        Node::InterpolatedStringNode { .. } | Node::ArrayNode { .. }
    )
}

fn source_of<'a>(node: &Node, ctx: &'a CheckContext) -> &'a str {
    let loc = node.location();
    &ctx.source[loc.start_offset()..loc.end_offset()]
}

/// A parsed format sequence
#[derive(Debug)]
struct FormatSeq {
    kind: FormatKind,
    arity: usize,
    is_percent: bool,
}

#[derive(Debug, PartialEq)]
enum FormatKind {
    Unnumbered,
    Numbered(usize),
    Named,
    Percent,
}

/// Parse format sequences from a Ruby format string source (including quotes).
fn parse_format_sequences(source: &str) -> Result<Vec<FormatSeq>, ()> {
    // Strip outer quotes/delimiters to get the raw content
    let content = strip_string_delimiters(source);

    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut sequences = Vec::new();

    while i < len {
        if bytes[i] == b'%' {
            i += 1;
            if i >= len {
                break;
            }

            // %% is a literal percent
            if bytes[i] == b'%' {
                sequences.push(FormatSeq {
                    kind: FormatKind::Percent,
                    arity: 0,
                    is_percent: true,
                });
                i += 1;
                continue;
            }

            // Named format: %{name} or %<name>
            if bytes[i] == b'{' {
                // Skip until closing }
                while i < len && bytes[i] != b'}' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                sequences.push(FormatSeq {
                    kind: FormatKind::Named,
                    arity: 1,
                    is_percent: false,
                });
                continue;
            }
            if bytes[i] == b'<' {
                // Skip until closing >
                while i < len && bytes[i] != b'>' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                // After > there should be a conversion char, skip it
                if i < len && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                sequences.push(FormatSeq {
                    kind: FormatKind::Named,
                    arity: 1,
                    is_percent: false,
                });
                continue;
            }

            // Check for flags: #, 0, -, +, space
            let mut arity: usize = 1;
            let mut has_star_width = false;
            let mut has_star_precision = false;

            // Flags
            while i < len && matches!(bytes[i], b'#' | b'0' | b'-' | b'+' | b' ') {
                i += 1;
            }

            // Width: can be digits, * (dynamic), or n$ (numbered)
            if i < len && bytes[i] == b'*' {
                has_star_width = true;
                i += 1;
            } else {
                // Check for numbered format: digits followed by $
                let digit_start = i;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < len && bytes[i] == b'$' && i > digit_start {
                    let num_str = std::str::from_utf8(&bytes[digit_start..i]).unwrap_or("0");
                    let num: usize = num_str.parse().unwrap_or(0);
                    i += 1; // skip $
                    // This is a numbered format sequence - parse remaining
                    // Skip flags again after number
                    while i < len && matches!(bytes[i], b'#' | b'0' | b'-' | b'+' | b' ') {
                        i += 1;
                    }
                    // Skip width
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    // Skip precision
                    if i < len && bytes[i] == b'.' {
                        i += 1;
                        while i < len && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    // Skip conversion char
                    if i < len && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    sequences.push(FormatSeq {
                        kind: FormatKind::Numbered(num),
                        arity: 1,
                        is_percent: false,
                    });
                    continue;
                }
                // Not numbered, it was just width digits, already consumed
            }

            // Precision
            if i < len && bytes[i] == b'.' {
                i += 1;
                if i < len && bytes[i] == b'*' {
                    has_star_precision = true;
                    i += 1;
                } else {
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }

            // Count arity for star width/precision
            if has_star_width {
                arity += 1;
            }
            if has_star_precision {
                arity += 1;
            }

            // Conversion character
            if i < len && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }

            sequences.push(FormatSeq {
                kind: FormatKind::Unnumbered,
                arity,
                is_percent: false,
            });
        } else if bytes[i] == b'#' && i + 1 < len && bytes[i + 1] == b'{' {
            // Ruby interpolation #{...} — skip entirely (can contain format-like chars)
            // We need to balance braces
            i += 2; // skip #{
            let mut depth = 1;
            while i < len && depth > 0 {
                if bytes[i] == b'{' {
                    depth += 1;
                } else if bytes[i] == b'}' {
                    depth -= 1;
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    Ok(sequences)
}

/// Check if format sequences mix numbered, unnumbered, and named types
fn has_mixed_format_types(sequences: &[FormatSeq]) -> bool {
    let mut has_numbered = false;
    let mut has_unnumbered = false;
    let mut has_named = false;

    for seq in sequences {
        match &seq.kind {
            FormatKind::Numbered(_) => has_numbered = true,
            FormatKind::Unnumbered => has_unnumbered = true,
            FormatKind::Named => has_named = true,
            FormatKind::Percent => {}
        }
    }

    let count = has_numbered as u8 + has_unnumbered as u8 + has_named as u8;
    count > 1
}

/// Count expected fields from format sequences
fn expected_fields_count(sequences: &[FormatSeq]) -> usize {
    // Check for named interpolation
    if sequences.iter().any(|s| s.kind == FormatKind::Named) {
        return 1;
    }

    // Check for numbered (digit dollar) format
    let max_dollar = sequences
        .iter()
        .filter_map(|s| {
            if let FormatKind::Numbered(n) = s.kind {
                Some(n)
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0);

    if max_dollar > 0 {
        return max_dollar;
    }

    // Count unnumbered sequences (excluding %%)
    sequences
        .iter()
        .filter(|s| !s.is_percent)
        .map(|s| s.arity)
        .sum()
}

/// Check if arguments don't match expected fields
fn matched_arguments_count(expected: usize, passed: usize) -> bool {
    expected != passed
}

/// Strip string delimiters from a Ruby string source
fn strip_string_delimiters(source: &str) -> &str {
    if source.starts_with('"') && source.ends_with('"') && source.len() >= 2 {
        &source[1..source.len() - 1]
    } else if source.starts_with('\'') && source.ends_with('\'') && source.len() >= 2 {
        &source[1..source.len() - 1]
    } else {
        source
    }
}
