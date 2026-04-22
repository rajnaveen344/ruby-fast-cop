//! Lint/AmbiguousOperator - Checks for ambiguous operators in unparenthesized method calls.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct AmbiguousOperator;

impl AmbiguousOperator {
    pub fn new() -> Self { Self }
}

const MSG_FORMAT: &str = "Ambiguous {actual} operator. Parenthesize the method \
    arguments if it's surely a {actual} operator, or add a whitespace to the \
    right of the `{op}` if it should be {possible}.";

struct Ambiguity {
    actual: &'static str,
    possible: &'static str,
    op: &'static str,
}

fn make_msg(amb: &Ambiguity) -> String {
    MSG_FORMAT
        .replace("{actual}", amb.actual)
        .replace("{op}", amb.op)
        .replace("{possible}", amb.possible)
}

impl Cop for AmbiguousOperator {
    fn name(&self) -> &'static str { "Lint/AmbiguousOperator" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        // Must not have parentheses
        if node.opening_loc().is_some() {
            return;
        }

        // Must not use safe navigation (`&.`)
        if node.call_operator_loc().map_or(false, |op| op.as_slice() == b"&.") {
            return;
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => {
                // No positional args — check block argument (for `&`)
                self.check_block_arg(node);
                return;
            }
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            self.check_block_arg(node);
            return;
        }

        let first_arg = &arg_list[0];
        let src = self.ctx.source;

        // Case 1: Splat node `*arr` — `puts *array` — the splat IS the first arg
        if let Some(splat) = first_arg.as_splat_node() {
            let op_start = splat.location().start_offset();
            let op_end = op_start + 1;
            let amb = Ambiguity { actual: "splat", possible: "a multiplication", op: "*" };
            self.add_offense(node, op_start, op_end, &make_msg(&amb));
            return;
        }

        // Case 2: KeywordHash with single AssocSplat `**kwargs`
        if let Some(kwhash) = first_arg.as_keyword_hash_node() {
            let elems: Vec<_> = kwhash.elements().iter().collect();
            if elems.len() == 1 {
                if let Some(assoc_splat) = elems[0].as_assoc_splat_node() {
                    let op_start = assoc_splat.location().start_offset();
                    let op_end = op_start + 2;
                    let amb = Ambiguity { actual: "keyword splat", possible: "an exponent", op: "**" };
                    self.add_offense(node, op_start, op_end, &make_msg(&amb));
                    return;
                }
            }
        }

        // Case 3: Unary +/- on integer/float argument
        // When Prism parses `foo +42`, it gives CallNode(args=[IntegerNode("+42")])
        // When it parses `foo + 42`, it gives CallNode(name="+", receiver=foo, args=[42])
        // So we only reach here with the ambiguous form
        let first_src = &src[first_arg.location().start_offset()..first_arg.location().end_offset()];
        if let Some(op_char) = first_src.chars().next() {
            if op_char == '+' || op_char == '-' {
                match first_arg {
                    Node::IntegerNode { .. } | Node::FloatNode { .. } => {
                        let op_start = first_arg.location().start_offset();
                        let op_end = op_start + 1;
                        let (actual, possible, op) = if op_char == '+' {
                            ("positive number", "an addition", "+")
                        } else {
                            ("negative number", "a subtraction", "-")
                        };
                        let amb = Ambiguity { actual, possible, op };
                        self.add_offense(node, op_start, op_end, &make_msg(&amb));
                        return;
                    }
                    _ => {}
                }
            }
        }

        // Also check block arg after positional args
        self.check_block_arg(node);
    }

    fn check_block_arg(&mut self, node: &ruby_prism::CallNode) {
        // Case: `2.times &process` — BlockArgumentNode as call's block
        if let Some(block) = node.block() {
            if let Some(block_arg) = block.as_block_argument_node() {
                let op_start = block_arg.location().start_offset();
                let op_end = op_start + 1;
                let amb = Ambiguity { actual: "block", possible: "a binary AND", op: "&" };
                self.add_offense(node, op_start, op_end, &make_msg(&amb));
            }
        }
    }

    fn add_offense(&mut self, call: &ruby_prism::CallNode, op_start: usize, op_end: usize, msg: &str) {
        let call_end = call.location().end_offset();

        // Correction: wrap args in parens
        // Insert `(` right after method name, `)` at end of call
        let selector_end = call.message_loc().map_or(op_start, |l| l.end_offset());

        let correction = Correction {
            edits: vec![
                Edit {
                    start_offset: selector_end,
                    end_offset: op_start,
                    replacement: "(".to_string(),
                },
                Edit {
                    start_offset: call_end,
                    end_offset: call_end,
                    replacement: ")".to_string(),
                },
            ],
        };

        let mut offense = self.ctx.offense_with_range(
            "Lint/AmbiguousOperator",
            msg,
            Severity::Warning,
            op_start,
            op_end,
        );
        offense.correction = Some(correction);
        self.offenses.push(offense);
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/AmbiguousOperator", |_cfg| Some(Box::new(AmbiguousOperator::new())));
