//! Style/IfWithBooleanLiteralBranches — `if cond; true; else; false; end` → `cond`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/if_with_boolean_literal_branches.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::offense::{Correction, Edit, Offense, Severity};
use crate::node_name;
use ruby_prism::Node;

const COP_NAME: &str = "Style/IfWithBooleanLiteralBranches";
const MSG_FOR_ELSIF: &str = "Use `else` instead of redundant `elsif` with boolean literal branches.";

const COMPARISON_METHODS: &[&str] = &["==", "!=", "===", "<", ">", "<=", ">=", "<=>", "=~", "!~"];

#[derive(Default)]
pub struct IfWithBooleanLiteralBranches {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl IfWithBooleanLiteralBranches {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { allowed_methods, allowed_patterns }
    }
}

impl Cop for IfWithBooleanLiteralBranches {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        // Need: condition that returns boolean; branches that are exactly true/false (in some order).
        let cond = node.predicate();
        if !returns_boolean(&cond, &self.allowed_methods, &self.allowed_patterns) {
            return vec![];
        }

        // Then-branch: must be a single true/false literal.
        let then_kind = single_bool_branch(node.statements().map(|s| s.as_node()).as_ref());
        let then_kind = match then_kind { Some(k) => k, None => return vec![] };

        // Else-branch: subsequent must be ElseNode containing single true/false (opposite).
        let Some(sub) = node.subsequent() else { return vec![] };
        let else_kind = match &sub {
            Node::ElseNode { .. } => {
                let en = sub.as_else_node().unwrap();
                single_bool_branch(en.statements().map(|s| s.as_node()).as_ref())
            }
            _ => None,
        };
        let else_kind = match else_kind { Some(k) => k, None => return vec![] };

        // Branches must differ.
        if then_kind == else_kind { return vec![] }

        let kw_loc = node.if_keyword_loc();
        let is_elsif = kw_loc.as_ref().map_or(false, |kw| {
            ctx.src(kw.start_offset(), kw.end_offset()) == "elsif"
        });

        // Skip if elsif AND has another elsif inside (multiple_elsif?). RuboCop's check is
        // `parent.if_type? && parent.elsif?`. We emulate by *only* skipping when the if-node
        // is an elsif's parent for *another* elsif. Without parent pointers the cleanest
        // proxy is: don't flag the inner elsif of a multi-elsif chain. We detect that by
        // looking at the body's surrounding chain via not having access. Simplification:
        // RuboCop's `multiple_elsif?` only triggers for inner elsif whose parent is also elsif.
        // We approximate by skipping any IfNode whose if_kw == "elsif" AND its `subsequent`
        // is itself another elsif (which means there are 2+ elsifs after it). That isn't
        // exactly right; instead we let the visitor below skip recursion when an elsif
        // already had two elsifs. Pragmatic: skip if we are an elsif AND own subsequent is
        // another elsif. (Only single-elsif case in fixture.)
        if is_elsif {
            // Skip when subsequent is itself an elsif (= we are in middle of multi-elsif).
            if let Some(s) = node.subsequent() {
                if let Some(inner) = s.as_if_node() {
                    if let Some(kw) = inner.if_keyword_loc() {
                        if ctx.src(kw.start_offset(), kw.end_offset()) == "elsif" {
                            return vec![];
                        }
                    }
                }
            }
            // Skip when preceded by another elsif in the same chain (= our parent is elsif).
            if let Some(kw) = &kw_loc {
                let our_col = ctx.col_of(kw.start_offset());
                let our_line_start = ctx.line_start(kw.start_offset());
                let bytes = ctx.source.as_bytes();
                let mut p = kw.start_offset();
                while p > 0 {
                    p -= 1;
                    if bytes[p] == b'\n' {
                        let line_start = p + 1;
                        if line_start >= our_line_start { continue; }
                        let candidate = line_start + our_col;
                        if candidate + 5 <= ctx.source.len()
                            && &ctx.source[candidate..candidate+5] == "elsif"
                            && ctx.source[line_start..candidate].bytes().all(|b| b == b' ' || b == b'\t')
                        {
                            return vec![];
                        }
                        // Stop scanning if we reach an `if` or `unless` at <= our indent (= chain root or earlier code).
                        let mut col = 0;
                        while line_start + col < ctx.source.len() {
                            let b = bytes[line_start + col];
                            if b != b' ' && b != b'\t' { break; }
                            col += 1;
                        }
                        if col <= our_col {
                            let rest = &ctx.source[line_start + col..];
                            if rest.starts_with("if ") || rest.starts_with("if\n")
                                || rest.starts_with("unless ") || rest.starts_with("unless\n") {
                                break;
                            }
                        }
                    }
                }
            }
        }

        let is_ternary = node.end_keyword_loc().is_none() && {
            let s = node.location().start_offset();
            !ctx.source[s..].starts_with("if") && !ctx.source[s..].starts_with("unless") && !ctx.source[s..].starts_with("elsif")
        };

        let is_unless = kw_loc.as_ref().map_or(false, |kw| {
            ctx.src(kw.start_offset(), kw.end_offset()) == "unless"
        });

        // Offense range + message.
        let (range_start, range_end, kw_label) = if is_ternary {
            let cond_end = cond.location().end_offset();
            let n_end = node.location().end_offset();
            (cond_end, n_end, "ternary operator".to_string())
        } else if let Some(kw) = kw_loc {
            (kw.start_offset(), kw.end_offset(), {
                let txt = ctx.src(kw.start_offset(), kw.end_offset()).to_string();
                format!("`{}`", txt)
            })
        } else {
            return vec![];
        };

        let message = if is_elsif {
            MSG_FOR_ELSIF.to_string()
        } else {
            format!("Remove redundant {} with boolean literal branches.", kw_label)
        };

        let mut offense = ctx.offense_with_range(COP_NAME, &message, Severity::Convention, range_start, range_end);

        // Build replacement condition.
        // opposite_condition? = (!unless && if_branch.false?) || (unless && if_branch.true?)
        let opposite = (!is_unless && then_kind == BoolKind::False)
            || (is_unless && then_kind == BoolKind::True);

        let cond_src = ctx.src(cond.location().start_offset(), cond.location().end_offset()).to_string();
        let needs_parens = require_parens(&cond);
        let replacement = if opposite {
            if needs_parens { format!("!({})", cond_src) } else { format!("!{}", cond_src) }
        } else {
            cond_src
        };

        // Apply correction.
        let n_start = node.location().start_offset();
        let n_end = node.location().end_offset();
        let edits = if is_elsif {
            // Compute end-of-else-block for elsif IfNode. We want to replace from `elsif` to end of
            // last branch, NOT including the parent's `end` keyword.
            // Find end of subsequent (else clause): if subsequent is ElseNode, use its end.
            //   Otherwise (no else), use end of statements.
            let elsif_end = if let Some(s) = node.subsequent() {
                if let Some(en) = s.as_else_node() {
                    if let Some(stmts) = en.statements() {
                        let body: Vec<_> = stmts.body().iter().collect();
                        if let Some(last) = body.last() {
                            // include trailing newline up to start of `end`
                            last.location().end_offset()
                        } else { en.location().end_offset() }
                    } else { en.location().end_offset() }
                } else {
                    s.location().end_offset()
                }
            } else if let Some(stmts) = node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                body.last().map(|n| n.location().end_offset()).unwrap_or(n_start)
            } else {
                n_start
            };
            let indent = if let Some(stmts) = node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(b) = body.first() {
                    " ".repeat(ctx.col_of(b.location().start_offset()))
                } else {
                    " ".repeat(ctx.col_of(n_start) + 2)
                }
            } else {
                " ".repeat(ctx.col_of(n_start) + 2)
            };
            vec![
                Edit { start_offset: n_start, end_offset: elsif_end, replacement: format!("else\n{}{}", indent, replacement) },
            ]
        } else {
            vec![Edit { start_offset: n_start, end_offset: n_end, replacement }]
        };

        offense = offense.with_correction(Correction { edits });
        vec![offense]
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        let cond = node.predicate();
        if !returns_boolean(&cond, &self.allowed_methods, &self.allowed_patterns) {
            return vec![];
        }

        let then_kind = single_bool_branch(node.statements().map(|s| s.as_node()).as_ref());
        let then_kind = match then_kind { Some(k) => k, None => return vec![] };

        let Some(en) = node.else_clause() else { return vec![] };
        let else_kind = match single_bool_branch(en.statements().map(|s| s.as_node()).as_ref()) {
            Some(k) => k, None => return vec![],
        };
        if then_kind == else_kind { return vec![] }

        let kw = node.keyword_loc();
        let range_start = kw.start_offset();
        let range_end = kw.end_offset();
        let message = format!("Remove redundant `unless` with boolean literal branches.");

        let mut offense = ctx.offense_with_range(COP_NAME, &message, Severity::Convention, range_start, range_end);

        // unless: opposite when then_kind == True
        let opposite = then_kind == BoolKind::True;
        let cond_src = ctx.src(cond.location().start_offset(), cond.location().end_offset()).to_string();
        let needs_parens = require_parens(&cond);
        let replacement = if opposite {
            if needs_parens { format!("!({})", cond_src) } else { format!("!{}", cond_src) }
        } else {
            cond_src
        };

        let n_start = node.location().start_offset();
        let n_end = node.location().end_offset();
        offense = offense.with_correction(Correction { edits: vec![
            Edit { start_offset: n_start, end_offset: n_end, replacement },
        ]});
        vec![offense]
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum BoolKind { True, False }

fn single_bool_branch(stmts: Option<&Node>) -> Option<BoolKind> {
    let s = stmts?;
    let sn = s.as_statements_node()?;
    let body: Vec<_> = sn.body().iter().collect();
    if body.len() != 1 { return None; }
    match &body[0] {
        Node::TrueNode { .. } => Some(BoolKind::True),
        Node::FalseNode { .. } => Some(BoolKind::False),
        _ => None,
    }
}

fn returns_boolean(cond: &Node, allowed_methods: &[String], allowed_patterns: &[String]) -> bool {
    match cond {
        Node::ParenthesesNode { .. } => {
            let p = cond.as_parentheses_node().unwrap();
            let Some(body) = p.body() else { return false };
            // Should be a StatementsNode with one child.
            if let Some(stmts) = body.as_statements_node() {
                let inner: Vec<_> = stmts.body().iter().collect();
                if inner.len() != 1 { return false; }
                returns_boolean(&inner[0], allowed_methods, allowed_patterns)
            } else {
                returns_boolean(&body, allowed_methods, allowed_patterns)
            }
        }
        Node::OrNode { .. } => {
            let o = cond.as_or_node().unwrap();
            returns_boolean(&o.left(), allowed_methods, allowed_patterns)
                && returns_boolean(&o.right(), allowed_methods, allowed_patterns)
        }
        Node::AndNode { .. } => {
            let a = cond.as_and_node().unwrap();
            returns_boolean(&a.right(), allowed_methods, allowed_patterns)
        }
        Node::CallNode { .. } => {
            let c = cond.as_call_node().unwrap();
            assume_boolean_call(&c, allowed_methods, allowed_patterns)
        }
        _ => false,
    }
}

fn assume_boolean_call(call: &ruby_prism::CallNode, allowed_methods: &[String], allowed_patterns: &[String]) -> bool {
    let method = node_name!(call);
    let m = method.as_ref();

    if is_method_allowed(allowed_methods, allowed_patterns, m, None) {
        return false;
    }
    if COMPARISON_METHODS.contains(&m) {
        return true;
    }
    if m.ends_with('?') {
        return true;
    }
    // Double negation: !!x → call.method = "!", receiver is call with method "!".
    if m == "!" {
        if let Some(recv) = call.receiver() {
            if let Some(inner) = recv.as_call_node() {
                let inner_m = node_name!(inner);
                if inner_m == "!" {
                    return true;
                }
            }
        }
    }
    false
}

fn require_parens(cond: &Node) -> bool {
    match cond {
        Node::AndNode { .. } | Node::OrNode { .. } => true,
        Node::CallNode { .. } => {
            let c = cond.as_call_node().unwrap();
            let m = node_name!(c);
            COMPARISON_METHODS.contains(&m.as_ref())
        }
        _ => false,
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

crate::register_cop!("Style/IfWithBooleanLiteralBranches", |cfg| {
    let c: Cfg = cfg.typed(COP_NAME);
    let mut am = c.allowed_methods;
    if am.is_empty() {
        am = vec!["nonzero?".to_string()];
    }
    Some(Box::new(IfWithBooleanLiteralBranches::with_config(am, c.allowed_patterns)))
});
