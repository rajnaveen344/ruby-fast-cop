//! Style/IfWithSemicolon — flag `if cond; body end` patterns.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/if_with_semicolon.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/IfWithSemicolon";

#[derive(Default)]
pub struct IfWithSemicolon;

impl IfWithSemicolon {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for IfWithSemicolon {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            suppress: false,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Mirrors RuboCop's `part_of_ignored_node?` — skip descendants of a
    /// node that was already flagged.
    suppress: bool,
}

/// Collect top-level statements in optional StatementsNode
fn body_list<'pr>(s: &Option<ruby_prism::StatementsNode<'pr>>) -> Vec<Node<'pr>> {
    s.as_ref()
        .map(|s| s.body().iter().collect())
        .unwrap_or_default()
}

fn is_masgn(n: &Node) -> bool {
    matches!(n, Node::MultiWriteNode { .. })
}

fn is_block(n: &Node) -> bool {
    // CallNode with block child, or stand-alone Block/Lambda nodes
    if let Some(c) = n.as_call_node() {
        return c.block().is_some();
    }
    matches!(
        n,
        Node::BlockNode { .. } | Node::LambdaNode { .. }
    )
}

fn is_return_with_args(n: &Node) -> bool {
    if let Some(r) = n.as_return_node() {
        return r.arguments().is_some();
    }
    false
}

/// Mirror of RuboCop's `require_argument_parentheses?`
fn require_argument_parentheses(src: &str, node: &Node) -> bool {
    let call = match node.as_call_node() {
        Some(c) => c,
        None => return false,
    };
    // arithmetic operators → skip
    let name_bytes = call.name().as_slice().to_vec();
    let name = String::from_utf8_lossy(&name_bytes);
    if matches!(name.as_ref(), "+" | "-" | "*" | "/" | "**" | "%" | ">>" | "<<" | "&" | "|" | "^" | "<=>" | "<" | "<=" | ">" | ">=") {
        return false;
    }
    // already parenthesized
    if call.opening_loc().is_some() {
        return false;
    }
    // no args
    let args = match call.arguments() {
        Some(a) => a,
        None => return false,
    };
    let arg_list: Vec<_> = args.arguments().iter().collect();
    if arg_list.is_empty() {
        return false;
    }
    // [] or []= methods
    if matches!(name.as_ref(), "[]" | "[]=") {
        return false;
    }
    // check it really looks like `method arg` (no `.` receiver call with parens)
    // If call has a receiver and call_operator, the source is `recv.method arg` —
    // we still need to wrap the args.
    let _ = src;
    true
}

/// Mirror of RuboCop's `build_expression` — wrap bare call args in parens.
fn build_expression(src: &str, node: &Node) -> String {
    if !require_argument_parentheses(src, node) {
        let loc = node.location();
        return src[loc.start_offset()..loc.end_offset()].to_string();
    }
    let call = node.as_call_node().unwrap();
    let args = call.arguments().unwrap();
    let arg_nodes: Vec<_> = args.arguments().iter().collect();
    let first_arg = &arg_nodes[0];

    // method part: from node start to message_loc end
    let node_start = node.location().start_offset();
    let msg_end = call
        .message_loc()
        .map(|m| m.end_offset())
        .unwrap_or(node_start);
    let method_src = &src[node_start..msg_end];

    // args part: from first_arg start to node end
    let args_start = first_arg.location().start_offset();
    let node_end = node.location().end_offset();
    let args_src = &src[args_start..node_end];

    format!("{}({})", method_src, args_src)
}

/// Mirror of RuboCop's `build_else_branch` — recursive elsif/else builder.
fn build_else_branch(src: &str, node: &ruby_prism::IfNode) -> String {
    let cond_loc = node.predicate().location();
    let cond_src = &src[cond_loc.start_offset()..cond_loc.end_offset()];

    let body_src = match node.statements() {
        Some(stmts) => {
            let loc = stmts.location();
            src[loc.start_offset()..loc.end_offset()].to_string()
        }
        None => String::new(),
    };

    let mut result = format!("elsif {}\n  {}\n", cond_src, body_src);

    match node.subsequent() {
        Some(Node::IfNode { .. }) => {
            let inner = node.subsequent().unwrap().as_if_node().unwrap();
            result += &build_else_branch(src, &inner);
        }
        Some(Node::ElseNode { .. }) => {
            let en = node.subsequent().unwrap().as_else_node().unwrap();
            let else_body = match en.statements() {
                Some(stmts) => {
                    let loc = stmts.location();
                    src[loc.start_offset()..loc.end_offset()].to_string()
                }
                None => String::new(),
            };
            result += &format!("else\n  {}\n", else_body);
        }
        _ => {}
    }

    result
}

/// Mirror of RuboCop's `correct_elsif` — build multi-line if/elsif/end.
fn correct_elsif(src: &str, node: &ruby_prism::IfNode) -> String {
    let cond_loc = node.predicate().location();
    let cond_src = &src[cond_loc.start_offset()..cond_loc.end_offset()];

    let body_src = match node.statements() {
        Some(stmts) => {
            let loc = stmts.location();
            src[loc.start_offset()..loc.end_offset()].to_string()
        }
        None => String::new(),
    };

    let elsif_node = node
        .subsequent()
        .unwrap()
        .as_if_node()
        .unwrap();

    let else_branch = build_else_branch(src, &elsif_node);
    // chop trailing newline from else_branch for the template
    let else_branch_chopped = else_branch.trim_end_matches('\n');

    format!(
        "if {}\n  {}\n{}\nend",
        cond_src, body_src, else_branch_chopped
    )
}

/// Mirror of RuboCop's `replacement` — produce ternary or elsif form.
fn replacement(src: &str, node: &ruby_prism::IfNode) -> String {
    // elsif case → multi-line form
    if let Some(subseq) = node.subsequent() {
        if subseq.as_if_node().is_some() {
            return correct_elsif(src, node);
        }
    }

    // ternary form
    let cond_loc = node.predicate().location();
    let cond_src = &src[cond_loc.start_offset()..cond_loc.end_offset()];

    let then_code = match node.statements() {
        Some(stmts) => {
            let nodes: Vec<_> = stmts.body().iter().collect();
            if nodes.is_empty() {
                "nil".to_string()
            } else {
                build_expression(src, &nodes[0])
            }
        }
        None => "nil".to_string(),
    };

    let else_code = match node.subsequent() {
        Some(Node::ElseNode { .. }) => {
            let en = node.subsequent().unwrap().as_else_node().unwrap();
            match en.statements() {
                Some(stmts) => {
                    let nodes: Vec<_> = stmts.body().iter().collect();
                    if nodes.is_empty() {
                        "nil".to_string()
                    } else {
                        build_expression(src, &nodes[0])
                    }
                }
                None => "nil".to_string(),
            }
        }
        _ => "nil".to_string(),
    };

    format!("{} ? {} : {}", cond_src, then_code, else_code)
}

impl<'a> Visitor<'a> {
    /// Collect each branch as a Vec of its top-level statements. A branch with
    /// multiple statements corresponds to a BeginNode in RuboCop's AST.
    fn collect_branch_stmts<'pr>(
        &self,
        node: &ruby_prism::IfNode<'pr>,
    ) -> Vec<Vec<Node<'pr>>> {
        let mut branches: Vec<Vec<Node<'pr>>> = Vec::new();
        branches.push(body_list(&node.statements()));
        match node.subsequent() {
            Some(Node::ElseNode { .. }) => {
                let en = node.subsequent().unwrap().as_else_node().unwrap();
                branches.push(body_list(&en.statements()));
            }
            Some(Node::IfNode { .. }) => {
                let inner = node.subsequent().unwrap().as_if_node().unwrap();
                branches.extend(self.collect_branch_stmts(&inner));
            }
            _ => {}
        }
        branches
    }

    fn has_elsif_or_begin_in_else(&self, node: &ruby_prism::IfNode) -> bool {
        match node.subsequent() {
            Some(Node::IfNode { .. }) => true, // elsif => if/else
            Some(Node::ElseNode { .. }) => {
                let en = node.subsequent().unwrap().as_else_node().unwrap();
                let body = body_list(&en.statements());
                body.len() > 1 // multi-stmt else = begin_type?
            }
            _ => false,
        }
    }

    fn check(&mut self, node: &ruby_prism::IfNode) -> bool {
        // Skip elsif (if_keyword_loc source starts with "elsif")
        if let Some(kw) = node.if_keyword_loc() {
            let kw_src = &self.ctx.source[kw.start_offset()..kw.end_offset()];
            if kw_src == "elsif" {
                return false;
            }
        } else {
            return false; // ternary / no keyword
        }

        // Must have end keyword (not a modifier)
        let end_kw = match node.end_keyword_loc() {
            Some(e) => e,
            None => return false,
        };

        // Detect `;` immediately after condition (before body). Scan between
        // predicate end and end-keyword start for a `;` that is the first
        // non-space char after the predicate.
        let pred_end = node.predicate().location().end_offset();
        let tail = &self.ctx.source[pred_end..end_kw.start_offset()];
        // Skip spaces/tabs
        let spaces_len = tail.len() - tail.trim_start_matches(|c: char| c == ' ' || c == '\t').len();
        let trimmed = &tail[spaces_len..];
        if !trimmed.starts_with(';') {
            return false;
        }
        // Absolute offset of the `;`
        let semi_offset = pred_end + spaces_len;

        // Build message
        let branch_stmts = self.collect_branch_stmts(node);
        // A branch with multiple statements = "begin_type?" in Ruby AST.
        let any_begin = branch_stmts.iter().any(|b| b.len() > 1);
        let flat: Vec<&Node> = branch_stmts.iter().flatten().collect();
        let any_return_args = flat.iter().any(|n| is_return_with_args(n));
        let any_masgn = flat.iter().any(|n| is_masgn(n));
        let any_block = flat.iter().any(|n| is_block(n));
        let else_is_if_or_begin = self.has_elsif_or_begin_in_else(node);

        let require_newline = any_begin || any_return_args;
        let require_if_else = !require_newline && (else_is_if_or_begin || any_masgn || any_block);

        // Read the condition source for the message
        let cond_src = {
            let p = node.predicate().location();
            &self.ctx.source[p.start_offset()..p.end_offset()]
        };
        let keyword = "if";

        let msg = if require_newline {
            format!(
                "Do not use `{} {};` - use a newline instead.",
                keyword, cond_src
            )
        } else if require_if_else {
            format!(
                "Do not use `{} {};` - use `if/else` instead.",
                keyword, cond_src
            )
        } else {
            format!(
                "Do not use `{} {};` - use a ternary operator instead.",
                keyword, cond_src
            )
        };

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        // Build correction
        let correction = if require_newline || (any_masgn || any_block) {
            // Replace just the semicolon with newline
            Correction {
                edits: vec![Edit {
                    start_offset: semi_offset,
                    end_offset: semi_offset + 1,
                    replacement: "\n".into(),
                }],
            }
        } else {
            // Replace entire node with ternary or multi-line if
            let new_src = replacement(self.ctx.source, node);
            Correction::replace(start, end, new_src)
        };

        let offense = self
            .ctx
            .offense_with_range(COP_NAME, &msg, Severity::Convention, start, end)
            .with_correction(correction);
        self.offenses.push(offense);
        true
    }
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        let was = self.suppress;
        if !self.suppress && self.check(node) {
            self.suppress = true;
        }
        ruby_prism::visit_if_node(self, node);
        self.suppress = was;
    }
}

crate::register_cop!("Style/IfWithSemicolon", |_cfg| {
    Some(Box::new(IfWithSemicolon::new()))
});
