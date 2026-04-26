//! Style/RedundantSort - prefer `min`/`max`/`min_by`/`max_by` over `sort.first/last` etc.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_sort.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{CallNode, Node};

#[derive(Default)]
pub struct RedundantSort;

impl RedundantSort {
    pub fn new() -> Self {
        Self
    }

    /// Parse an integer node source as i64.
    fn int_value(source: &str, node: &Node) -> Option<i64> {
        if !matches!(node, Node::IntegerNode { .. }) {
            return None;
        }
        let loc = node.location();
        source[loc.start_offset()..loc.end_offset()]
            .chars()
            .filter(|c| *c != '_')
            .collect::<String>()
            .parse::<i64>()
            .ok()
    }

    /// Given the outer accessor CallNode (e.g. `.first`, `[0]`, `.at(0)`, `.slice(-1)`),
    /// extract (accessor_symbol, arg_value_if_any).
    /// Returns None if not a recognized accessor pattern.
    fn classify_accessor(node: &CallNode, source: &str) -> Option<(&'static str, Option<i64>)> {
        let method = node_name!(node);
        let method_str: &str = method.as_ref();

        // Must have no block
        if node.block().is_some() {
            return None;
        }

        let args: Vec<Node> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => vec![],
        };

        match method_str {
            "first" | "last" => {
                if !args.is_empty() {
                    return None;
                }
                let sym = if method_str == "first" { "first" } else { "last" };
                Some((sym, None))
            }
            "[]" | "at" | "slice" => {
                if args.len() != 1 {
                    return None;
                }
                let v = Self::int_value(source, &args[0])?;
                if v != 0 && v != -1 {
                    return None;
                }
                let sym = match method_str {
                    "[]" => "[]",
                    "at" => "at",
                    _ => "slice",
                };
                Some((sym, Some(v)))
            }
            _ => None,
        }
    }

    /// Unwrap BlockNode wrapper: if `node` is a CallNode whose receiver is a CallNode with
    /// a block, return that inner call; otherwise return the call as-is if possible.
    /// The returned call is what we inspect to determine sort/sort_by.
    ///
    /// Accepts: `x.sort`, `x.sort_by(&:foo)`, `x.sort { |a,b| ... }`, `x.sort_by { |x| ... }`.
    /// Returns Some((sort_call, sorter_name_lowercase)) if receiver is a redundant sort.
    fn extract_sort<'a>(receiver: &Node<'a>) -> Option<(CallNode<'a>, &'static str)> {
        // Case 1: receiver is directly a CallNode
        if let Some(call) = receiver.as_call_node() {
            // If the call has a block attached, the block is inside the same CallNode
            let name = node_name!(call);
            let name_str: &str = name.as_ref();
            match name_str {
                "sort" => {
                    // sort: must have no arguments (mongo case excluded)
                    let has_args = call
                        .arguments()
                        .map(|a| a.arguments().iter().count() > 0)
                        .unwrap_or(false);
                    if has_args {
                        return None;
                    }
                    Some((call, "sort"))
                }
                "sort_by" => {
                    // sort_by: must have either an argument (e.g., &:foo) or a block
                    let has_args = call
                        .arguments()
                        .map(|a| a.arguments().iter().count() > 0)
                        .unwrap_or(false);
                    let has_block = call.block().is_some();
                    if !has_args && !has_block {
                        return None;
                    }
                    Some((call, "sort_by"))
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Detect logical operator after `outer_end` in source.
    /// Returns Some((op_start, op_str, rhs_start, rhs_end)) or None.
    /// Only matches ` || `, ` && `, ` or `, ` and ` (with surrounding spaces).
    fn detect_logical_op(source: &str, outer_end: usize) -> Option<(usize, &'static str, usize, usize)> {
        let src = source.as_bytes();
        let len = source.len();
        if outer_end >= len {
            return None;
        }
        // Skip horizontal whitespace (spaces/tabs but not newline)
        let mut i = outer_end;
        while i < len && (src[i] == b' ' || src[i] == b'\t') {
            i += 1;
        }
        // Match operator
        let (op_start, op_str, after_op) = if src[i..].starts_with(b"||") {
            (i, "||", i + 2)
        } else if src[i..].starts_with(b"&&") {
            (i, "&&", i + 2)
        } else if src[i..].starts_with(b"or") && (i + 2 >= len || !src[i+2].is_ascii_alphanumeric() && src[i+2] != b'_') {
            (i, "or", i + 2)
        } else if src[i..].starts_with(b"and") && (i + 3 >= len || !src[i+3].is_ascii_alphanumeric() && src[i+3] != b'_') {
            (i, "and", i + 3)
        } else {
            return None;
        };
        // Skip whitespace after operator
        let mut rhs_start = after_op;
        while rhs_start < len && (src[rhs_start] == b' ' || src[rhs_start] == b'\t') {
            rhs_start += 1;
        }
        // RHS extends to end of line (or end of input)
        let rhs_end = source[rhs_start..].find('\n').map(|n| rhs_start + n).unwrap_or(len);
        if rhs_start >= rhs_end {
            return None;
        }
        Some((op_start, op_str, rhs_start, rhs_end))
    }

    fn suggestion(sorter: &str, accessor: &str, arg: Option<i64>) -> String {
        let base = match accessor {
            "first" => "min",
            "last" => "max",
            _ => match arg {
                Some(0) => "min",
                Some(-1) => "max",
                _ => return String::new(),
            },
        };
        let suffix = if sorter == "sort_by" { "_by" } else { "" };
        format!("{}{}", base, suffix)
    }
}

impl Cop for RedundantSort {
    fn name(&self) -> &'static str {
        "Style/RedundantSort"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // We match the outer accessor call (.first, .last, [0], [-1], .at(..), .slice(..)).
        let (accessor, arg) = match Self::classify_accessor(node, ctx.source) {
            Some(x) => x,
            None => return vec![],
        };

        // Receiver must be a sort/sort_by call (directly).
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let (sort_call, sorter) = match Self::extract_sort(&receiver) {
            Some(x) => x,
            None => return vec![],
        };

        // Offense range: from sort's selector (message_loc) start to outer node's end.
        let sort_sel = match sort_call.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let start_offset = sort_sel.start_offset();
        let end_offset = node.location().end_offset();

        // Accessor source for the message: from outer call's message_loc start to outer end.
        let outer_msg = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let accessor_source = &ctx.source[outer_msg.start_offset()..end_offset];

        let suggestion = Self::suggestion(sorter, accessor, arg);
        if suggestion.is_empty() {
            return vec![];
        }

        let message = format!(
            "Use `{}` instead of `{}...{}`.",
            suggestion, sorter, accessor_source
        );

        // Correction: replace sort/sort_by selector + delete accessor call
        // e.g. `arr.sort.first` → `arr.min`
        // Multi-edit: (1) replace sort_sel with suggestion, (2) delete accessor (from its dot to end)
        // For accessor deletion, use accessor_start = dot position (not end of sort_call)
        let accessor_start = if let Some(dot_loc) = node.call_operator_loc() {
            dot_loc.start_offset()
        } else if let Some(msg_loc) = node.message_loc() {
            msg_loc.start_offset()
        } else {
            sort_call.location().end_offset()
        };

        // Check for logical operator after the accessor: `.first || []` → ` ||\n    []`
        // RuboCop's `replace_with_logical_operator` pattern.
        let sort_call_end = sort_call.location().end_offset();
        let correction = if let Some((op_start, op_str, rhs_start, rhs_end)) =
            Self::detect_logical_op(ctx.source, node.location().end_offset())
        {
            // Determine indent: find the column of the sort_by receiver's first non-space char.
            // Use sort_sel line start to find column.
            let line_start = ctx.source[..sort_sel.start_offset()]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);
            let base_indent = sort_sel.start_offset() - line_start - 1; // -1 for the dot
            let indent = " ".repeat(base_indent + 2);
            let rhs_src = &ctx.source[rhs_start..rhs_end];
            // Replace accessor+operator+rhs with ` op\n{indent}rhs`
            let tail_replacement = format!(" {}\n{}{}", op_str, indent, rhs_src);
            Correction {
                edits: vec![
                    Edit {
                        start_offset: sort_sel.start_offset(),
                        end_offset: sort_sel.end_offset(),
                        replacement: suggestion.clone(),
                    },
                    Edit {
                        start_offset: sort_call_end,
                        end_offset: rhs_end,
                        replacement: tail_replacement,
                    },
                ],
            }
        } else {
            Correction {
                edits: vec![
                    Edit {
                        start_offset: sort_sel.start_offset(),
                        end_offset: sort_sel.end_offset(),
                        replacement: suggestion.clone(),
                    },
                    Edit {
                        start_offset: accessor_start,
                        end_offset: node.location().end_offset(),
                        replacement: String::new(),
                    },
                ],
            }
        };

        let offense =
            ctx.offense_with_range(self.name(), &message, self.severity(), start_offset, end_offset)
                .with_correction(correction);
        vec![offense]
    }
}

crate::register_cop!("Style/RedundantSort", |_cfg| {
    Some(Box::new(RedundantSort::new()))
});
