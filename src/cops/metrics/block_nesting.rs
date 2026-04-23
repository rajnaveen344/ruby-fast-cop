//! Metrics/BlockNesting cop
//! Checks for excessive nesting of conditional and looping constructs.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/metrics/block_nesting.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Cfg {
    #[serde(default = "default_max")]
    max: usize,
    #[serde(default)]
    count_blocks: bool,
    #[serde(default)]
    count_modifier_forms: bool,
}

fn default_max() -> usize { 3 }

impl Default for Cfg {
    fn default() -> Self {
        Cfg { max: 3, count_blocks: false, count_modifier_forms: false }
    }
}

pub struct BlockNesting {
    max: usize,
    count_blocks: bool,
    count_modifier_forms: bool,
}

impl BlockNesting {
    pub fn new(max: usize, count_blocks: bool, count_modifier_forms: bool) -> Self {
        Self { max, count_blocks, count_modifier_forms }
    }
}

impl Cop for BlockNesting {
    fn name(&self) -> &'static str { "Metrics/BlockNesting" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut checker = BlockNestingChecker {
            source: ctx.source,
            max: self.max,
            count_blocks: self.count_blocks,
            count_modifier_forms: self.count_modifier_forms,
            offenses: Vec::new(),
            ctx,
            depth: 0,
            // Track nodes that have already triggered an offense so we don't cascade
            offending_starts: Vec::new(),
        };
        let result = ruby_prism::parse(ctx.source.as_bytes());
        checker.visit(&result.node());
        checker.offenses
    }
}

struct BlockNestingChecker<'a> {
    source: &'a str,
    max: usize,
    count_blocks: bool,
    count_modifier_forms: bool,
    offenses: Vec<Offense>,
    ctx: &'a CheckContext<'a>,
    depth: usize,
    offending_starts: Vec<usize>,
}

impl<'a> BlockNestingChecker<'a> {
    fn emit_offense(&mut self, start: usize, end: usize) {
        let msg = format!("Avoid more than {} levels of block nesting.", self.max);
        self.offenses.push(self.ctx.offense_with_range(
            "Metrics/BlockNesting", &msg, Severity::Convention,
            start, end,
        ));
        self.offending_starts.push(start);
    }

    fn already_offending(&self, start: usize) -> bool {
        self.offending_starts.contains(&start)
    }
}

impl Visit<'_> for BlockNestingChecker<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Detect elsif by checking for "elsif" keyword
        // IfNode representing elsif has if_keyword_loc returning the "elsif" keyword
        let is_elsif = if let Some(kw_loc) = node.if_keyword_loc() {
            &self.source[kw_loc.start_offset()..kw_loc.end_offset()] == "elsif"
        } else {
            false
        };

        // elsif doesn't add a nesting level
        if is_elsif {
            ruby_prism::visit_if_node(self, node);
            return;
        }

        // Check if modifier form: no `then` keyword and no `end` keyword
        let is_modifier = node.then_keyword_loc().is_none() && node.end_keyword_loc().is_none();
        let counts = if is_modifier { self.count_modifier_forms } else { true };

        if counts {
            self.depth += 1;
            if self.depth > self.max {
                let loc = node.location();
                if !self.already_offending(loc.start_offset()) {
                    self.emit_offense(loc.start_offset(), loc.end_offset());
                }
                // Don't recurse into children of offending node
                self.depth -= 1;
                return;
            }
        }

        ruby_prism::visit_if_node(self, node);

        if counts {
            self.depth -= 1;
        }
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        // unless is like if but not an elsif
        let is_modifier = node.then_keyword_loc().is_none() && node.end_keyword_loc().is_none();
        let counts = if is_modifier { self.count_modifier_forms } else { true };

        if counts {
            self.depth += 1;
            if self.depth > self.max {
                let loc = node.location();
                if !self.already_offending(loc.start_offset()) {
                    self.emit_offense(loc.start_offset(), loc.end_offset());
                }
                self.depth -= 1;
                return;
            }
        }

        ruby_prism::visit_unless_node(self, node);

        if counts {
            self.depth -= 1;
        }
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        // begin...end while: is_begin_modifier()=true → always count (it's while_post, not modifier)
        // x while cond: body offset < condition offset → modifier form
        // while cond; body; end: regular → always count
        let is_begin_mod = node.is_begin_modifier();
        let is_postfix_modifier = !is_begin_mod && if let Some(body) = node.statements() {
            body.location().start_offset() < node.predicate().location().start_offset()
        } else {
            false
        };
        let counts = if is_postfix_modifier { self.count_modifier_forms } else { true };

        if counts {
            self.depth += 1;
            if self.depth > self.max {
                let loc = node.location();
                let start = loc.start_offset();
                if !self.already_offending(start) {
                    // For begin...end while: offense on "begin" keyword (5 chars)
                    let end = if is_begin_mod { start + 5 } else { loc.end_offset() };
                    self.emit_offense(start, end);
                }
                self.depth -= 1;
                return;
            }
        }

        ruby_prism::visit_while_node(self, node);

        if counts {
            self.depth -= 1;
        }
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let is_begin_mod = node.is_begin_modifier();
        let is_postfix_modifier = !is_begin_mod && if let Some(body) = node.statements() {
            body.location().start_offset() < node.predicate().location().start_offset()
        } else {
            false
        };
        let counts = if is_postfix_modifier { self.count_modifier_forms } else { true };

        if counts {
            self.depth += 1;
            if self.depth > self.max {
                let loc = node.location();
                let start = loc.start_offset();
                if !self.already_offending(start) {
                    let end = if is_begin_mod { start + 5 } else { loc.end_offset() };
                    self.emit_offense(start, end);
                }
                self.depth -= 1;
                return;
            }
        }

        ruby_prism::visit_until_node(self, node);

        if counts {
            self.depth -= 1;
        }
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        self.depth += 1;
        if self.depth > self.max {
            let loc = node.location();
            if !self.already_offending(loc.start_offset()) {
                self.emit_offense(loc.start_offset(), loc.end_offset());
            }
            self.depth -= 1;
            return;
        }
        ruby_prism::visit_for_node(self, node);
        self.depth -= 1;
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        self.depth += 1;
        if self.depth > self.max {
            let loc = node.location();
            if !self.already_offending(loc.start_offset()) {
                self.emit_offense(loc.start_offset(), loc.end_offset());
            }
            self.depth -= 1;
            return;
        }
        ruby_prism::visit_case_node(self, node);
        self.depth -= 1;
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        self.depth += 1;
        if self.depth > self.max {
            let loc = node.location();
            if !self.already_offending(loc.start_offset()) {
                self.emit_offense(loc.start_offset(), loc.end_offset());
            }
            self.depth -= 1;
            return;
        }
        ruby_prism::visit_case_match_node(self, node);
        self.depth -= 1;
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        self.depth += 1;
        if self.depth > self.max {
            let loc = node.location();
            if !self.already_offending(loc.start_offset()) {
                self.emit_offense(loc.start_offset(), loc.end_offset());
            }
            self.depth -= 1;
            return;
        }
        ruby_prism::visit_rescue_node(self, node);
        self.depth -= 1;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // When CountBlocks is enabled, a call with a block counts as a nesting level.
        // We handle the block at the CallNode level so we can report on the CallNode location.
        if self.count_blocks && node.block().is_some() {
            self.depth += 1;
            if self.depth > self.max {
                let loc = node.location();
                if !self.already_offending(loc.start_offset()) {
                    self.emit_offense(loc.start_offset(), loc.end_offset());
                }
                self.depth -= 1;
                // Don't recurse into the block
                return;
            }
            // Recurse into receiver and arguments but NOT the block (we track block as nesting)
            // Actually we need to recurse fully but the block itself shouldn't double-count
            ruby_prism::visit_call_node(self, node);
            self.depth -= 1;
        } else {
            ruby_prism::visit_call_node(self, node);
        }
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // Block handling is done at CallNode level when count_blocks is true.
        // When count_blocks is false, just recurse without incrementing depth.
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        if !self.count_blocks {
            ruby_prism::visit_lambda_node(self, node);
            return;
        }
        self.depth += 1;
        if self.depth > self.max {
            let loc = node.location();
            if !self.already_offending(loc.start_offset()) {
                self.emit_offense(loc.start_offset(), loc.end_offset());
            }
            self.depth -= 1;
            return;
        }
        ruby_prism::visit_lambda_node(self, node);
        self.depth -= 1;
    }
}

crate::register_cop!("Metrics/BlockNesting", |cfg| {
    let c: Cfg = cfg.typed("Metrics/BlockNesting");
    Some(Box::new(BlockNesting::new(c.max, c.count_blocks, c.count_modifier_forms)))
});
