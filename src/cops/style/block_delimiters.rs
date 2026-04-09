//! Style/BlockDelimiters cop
//!
//! Checks for uses of braces or do/end around single line or multi-line blocks.
//! Supports EnforcedStyle: line_count_based (default), semantic, braces_for_chaining, always_braces.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnforcedStyle {
    LineCountBased,
    Semantic,
    BracesForChaining,
    AlwaysBraces,
}

pub struct BlockDelimiters {
    enforced_style: EnforcedStyle,
    allow_braces_on_procedural_one_liners: bool,
    braces_required_methods: Vec<String>,
    functional_methods: HashSet<String>,
    procedural_methods: HashSet<String>,
    allowed_methods: HashSet<String>,
    allowed_patterns: Vec<String>,
}

impl BlockDelimiters {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self {
            enforced_style,
            allow_braces_on_procedural_one_liners: false,
            braces_required_methods: Vec::new(),
            functional_methods: HashSet::new(),
            procedural_methods: HashSet::new(),
            allowed_methods: HashSet::from([
                "lambda".to_string(),
                "proc".to_string(),
                "it".to_string(),
            ]),
            allowed_patterns: Vec::new(),
        }
    }

    pub fn with_config(
        enforced_style: EnforcedStyle,
        allow_braces_on_procedural_one_liners: bool,
        braces_required_methods: Vec<String>,
        functional_methods: Vec<String>,
        procedural_methods: Vec<String>,
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self {
            enforced_style,
            allow_braces_on_procedural_one_liners,
            braces_required_methods,
            functional_methods: functional_methods.into_iter().collect(),
            procedural_methods: procedural_methods.into_iter().collect(),
            allowed_methods: allowed_methods.into_iter().collect(),
            allowed_patterns,
        }
    }
}

impl Default for BlockDelimiters {
    fn default() -> Self {
        Self::new(EnforcedStyle::LineCountBased)
    }
}

// ── Visitor to collect block offenses ──

struct BlockVisitor<'a> {
    cop: &'a BlockDelimiters,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Byte offsets of block opening_loc that should be ignored
    /// (blocks that are arguments to unparenthesized method calls)
    ignored_blocks: HashSet<usize>,
    /// Ranges (start, end) of blocks that received offenses.
    /// Descendant blocks within these ranges are skipped (RuboCop's ignore_node behavior).
    offended_ranges: Vec<(usize, usize)>,
}

impl<'a> BlockVisitor<'a> {
    fn new(cop: &'a BlockDelimiters, ctx: &'a CheckContext<'a>) -> Self {
        Self {
            cop,
            ctx,
            offenses: Vec::new(),
            ignored_blocks: HashSet::new(),
            offended_ranges: Vec::new(),
        }
    }

    fn is_braces(&self, block_open_offset: usize) -> bool {
        self.ctx.source.as_bytes().get(block_open_offset) == Some(&b'{')
    }

    fn is_multiline(&self, start_offset: usize, end_offset: usize) -> bool {
        self.ctx.source[start_offset..end_offset].contains('\n')
    }

    fn method_name_str(&self, call: &ruby_prism::CallNode) -> String {
        node_name!(call).to_string()
    }

    fn is_allowed_method(&self, name: &str) -> bool {
        self.cop.allowed_methods.contains(name)
    }

    fn matches_allowed_pattern(&self, name: &str) -> bool {
        self.cop.allowed_patterns.iter().any(|pat| name.contains(pat))
    }

    fn is_braces_required_method(&self, name: &str) -> bool {
        self.cop.braces_required_methods.iter().any(|m| m == name)
    }

    /// Check if this block is inside a block that already got an offense
    fn part_of_offended_block(&self, start: usize, end: usize) -> bool {
        self.offended_ranges
            .iter()
            .any(|&(rs, re)| rs <= start && end <= re)
    }

    // ── on_send: collect ignored blocks ──

    fn collect_ignored_from_call(&mut self, call: &ruby_prism::CallNode) {
        if call.opening_loc().is_some() {
            return; // parenthesized
        }
        if call.is_attribute_write() {
            return;
        }
        if self.is_single_arg_operator_method(call) {
            return;
        }
        // Also skip if call has no arguments (block-only calls)
        if call.arguments().is_none() {
            return;
        }
        let arg_list: Vec<_> = if let Some(args) = call.arguments() {
            args.arguments().iter().collect()
        } else {
            return;
        };
        for arg in &arg_list {
            self.collect_blocks_from_node(arg);
        }
    }

    fn is_single_arg_operator_method(&self, call: &ruby_prism::CallNode) -> bool {
        let name = self.method_name_str(call);
        if !is_operator_method(&name) {
            return false;
        }
        if let Some(args) = call.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1 {
                return matches!(&arg_list[0], Node::BlockNode { .. });
            }
        }
        false
    }

    fn collect_blocks_from_node(&mut self, node: &Node) {
        match node {
            Node::BlockNode { .. } => {
                let block = node.as_block_node().unwrap();
                self.ignored_blocks
                    .insert(block.opening_loc().start_offset());
            }
            Node::CallNode { .. } => {
                if let Some(call) = node.as_call_node() {
                    if let Some(recv) = call.receiver() {
                        self.collect_blocks_from_node(&recv);
                    }
                    if let Some(args) = call.arguments() {
                        for arg in args.arguments().iter() {
                            self.collect_blocks_from_node(&arg);
                        }
                    }
                    if let Some(block) = call.block() {
                        self.collect_blocks_from_node(&block);
                    }
                }
            }
            Node::HashNode { .. } => {
                let hash = node.as_hash_node().unwrap();
                let has_braces = hash.opening_loc().as_slice() == b"{";
                if !has_braces {
                    for child in hash.elements().iter() {
                        self.collect_blocks_from_node(&child);
                    }
                }
            }
            Node::KeywordHashNode { .. } => {
                let kh = node.as_keyword_hash_node().unwrap();
                for child in kh.elements().iter() {
                    self.collect_blocks_from_node(&child);
                }
            }
            Node::AssocNode { .. } => {
                let assoc = node.as_assoc_node().unwrap();
                self.collect_blocks_from_node(&assoc.key());
                self.collect_blocks_from_node(&assoc.value());
            }
            _ => {}
        }
    }

    // ── on_block: check block style ──

    fn check_block(&mut self, call: &ruby_prism::CallNode, block: &ruby_prism::BlockNode) {
        let open_start = block.opening_loc().start_offset();
        let block_end = block.closing_loc().end_offset();

        // Skip if inside an already-offended block
        if self.part_of_offended_block(open_start, block_end) {
            return;
        }

        // Skip if this block is "ignored" (argument to unparenthesized call)
        if self.ignored_blocks.contains(&open_start) {
            return;
        }

        let method_name = self.method_name_str(call);

        // Skip allowed methods/patterns
        if self.is_allowed_method(&method_name) || self.matches_allowed_pattern(&method_name) {
            return;
        }

        // require_do_end? -- single-line do..end with rescue that has array exception type
        // OR single-line do..end with semicolon-separated rescue
        if self.require_do_end(block) {
            return;
        }

        // BracesRequiredMethods take precedence
        if self.is_braces_required_method(&method_name) {
            let braces = self.is_braces(open_start);
            if !braces {
                let msg = format!(
                    "Brace delimiters `{{...}}` required for '{}' method.",
                    method_name
                );
                self.add_offense(block, &msg);
            }
            return;
        }

        if self.proper_block_style(call, block) {
            return;
        }

        let msg = self.message(call, block);
        self.add_offense(block, &msg);
    }

    fn require_do_end(&self, block: &ruby_prism::BlockNode) -> bool {
        let braces = self.is_braces(block.opening_loc().start_offset());
        if braces {
            return false;
        }
        let multiline = self.is_multiline(
            block.opening_loc().start_offset(),
            block.closing_loc().end_offset(),
        );
        if multiline {
            return false;
        }
        // Single-line do..end block -- check if it has rescue with array exception type
        // OR has a semicolon-separated rescue clause
        self.has_problematic_rescue(block)
    }

    fn has_problematic_rescue(&self, block: &ruby_prism::BlockNode) -> bool {
        if let Some(body) = block.body() {
            return self.node_has_problematic_rescue(&body);
        }
        false
    }

    fn node_has_problematic_rescue(&self, node: &Node) -> bool {
        match node {
            Node::BeginNode { .. } => {
                let begin = node.as_begin_node().unwrap();
                if let Some(rescue_node) = begin.rescue_clause() {
                    // Check if it's a rescue with array exception type
                    if self.rescue_has_array(&rescue_node) {
                        return true;
                    }
                    // Check for semicolon before rescue in single-line block
                    // `foo do next unless bar; rescue StandardError; end`
                    // The semicolon before rescue means changing to braces would be invalid
                    let rescue_loc = rescue_node.location();
                    let block_src = &self.ctx.source
                        [node.location().start_offset()..rescue_loc.start_offset()];
                    if block_src.contains(';') {
                        return true;
                    }
                }
                false
            }
            Node::RescueNode { .. } => {
                self.rescue_has_array(&node.as_rescue_node().unwrap())
            }
            _ => false,
        }
    }

    fn rescue_has_array(&self, rescue: &ruby_prism::RescueNode) -> bool {
        let exceptions: Vec<_> = rescue.exceptions().iter().collect();
        if !exceptions.is_empty() {
            return matches!(&exceptions[0], Node::ArrayNode { .. });
        }
        false
    }

    fn proper_block_style(
        &self,
        call: &ruby_prism::CallNode,
        block: &ruby_prism::BlockNode,
    ) -> bool {
        let open_offset = block.opening_loc().start_offset();
        let close_offset = block.closing_loc().end_offset();
        let braces = self.is_braces(open_offset);
        let multiline = self.is_multiline(open_offset, close_offset);

        match self.cop.enforced_style {
            EnforcedStyle::LineCountBased => multiline ^ braces,
            EnforcedStyle::Semantic => {
                self.semantic_block_style(call, block, braces, multiline)
            }
            EnforcedStyle::BracesForChaining => {
                self.braces_for_chaining_style(call, block, braces, multiline)
            }
            EnforcedStyle::AlwaysBraces => braces,
        }
    }

    fn semantic_block_style(
        &self,
        call: &ruby_prism::CallNode,
        block: &ruby_prism::BlockNode,
        braces: bool,
        multiline: bool,
    ) -> bool {
        let method_name = self.method_name_str(call);
        if braces {
            self.cop.functional_methods.contains(&method_name)
                || self.functional_block(call, block)
                || (self.cop.allow_braces_on_procedural_one_liners && !multiline)
        } else {
            self.cop.procedural_methods.contains(&method_name)
                || !self.return_value_used(call, block)
        }
    }

    fn braces_for_chaining_style(
        &self,
        call: &ruby_prism::CallNode,
        block: &ruby_prism::BlockNode,
        braces: bool,
        multiline: bool,
    ) -> bool {
        if multiline {
            let chained = self.is_chained(call, block);
            if chained {
                braces
            } else {
                !braces
            }
        } else {
            braces
        }
    }

    /// Check if the block is chained (`.method`, `&.method`, or `[...]` after closing)
    fn is_chained(&self, _call: &ruby_prism::CallNode, block: &ruby_prism::BlockNode) -> bool {
        crate::helpers::source::is_chained_after(self.ctx.source, block.closing_loc().end_offset())
    }

    fn functional_block(
        &self,
        call: &ruby_prism::CallNode,
        block: &ruby_prism::BlockNode,
    ) -> bool {
        self.return_value_used(call, block) || self.return_value_of_scope(call, block)
    }

    /// Check if the return value of the call+block is used.
    /// This checks for: assignments, being passed to another method, chaining, parenthesized args.
    fn return_value_used(
        &self,
        call: &ruby_prism::CallNode,
        block: &ruby_prism::BlockNode,
    ) -> bool {
        // Check if chained -- chaining means the value is used
        if self.is_chained(call, block) {
            return true;
        }

        // Check the source before the outermost call start
        let call_start = self.outermost_call_start(call);
        let before = &self.ctx.source[..call_start];
        let trimmed = before.trim_end();

        // Assignment: `foo = `, `foo.bar = `, `foo += `, etc.
        if trimmed.ends_with('=') && !trimmed.ends_with("==") && !trimmed.ends_with("!=") {
            return true;
        }

        // Inside parentheses as argument: `puts (map do ... end)`
        if trimmed.ends_with('(') {
            return true;
        }

        // After the block close, check for operators or method calls
        let block_end = block.closing_loc().end_offset();
        let after = &self.ctx.source[block_end..];
        let trimmed_after = after.trim_start();

        // Operator: `+ something`, `<< something`, etc.
        if !trimmed_after.is_empty() {
            let first_char = trimmed_after.as_bytes()[0];
            if first_char == b'+' || first_char == b'-' || first_char == b'*'
                || first_char == b'/' || first_char == b'%'
            {
                return true;
            }
        }

        false
    }

    /// Check if the block is the return value of its scope.
    /// This checks for: conditionals, logical operators, arrays, ranges,
    /// and being the last expression in a method/block/lambda body.
    fn return_value_of_scope(
        &self,
        call: &ruby_prism::CallNode,
        block: &ruby_prism::BlockNode,
    ) -> bool {
        let call_start = self.outermost_call_start(call);
        let block_end = block.closing_loc().end_offset();
        let before = &self.ctx.source[..call_start];
        let trimmed_before = before.trim_end();
        let after = &self.ctx.source[block_end..];
        let trimmed_after = after.trim_start();

        // Check for logical operators after block
        if trimmed_after.starts_with("||") || trimmed_after.starts_with("&&") {
            return true;
        }
        // Check for range operators
        if trimmed_after.starts_with("..") {
            return true;
        }

        // Check for being inside an array literal
        if trimmed_before.ends_with('[') {
            return true;
        }

        // Check for conditional keywords before the call
        // `if any? { |x| x }` / `return if any? { ... }` etc.
        if ends_with_keyword(trimmed_before, "if")
            || ends_with_keyword(trimmed_before, "unless")
            || ends_with_keyword(trimmed_before, "while")
            || ends_with_keyword(trimmed_before, "until")
            || ends_with_keyword(trimmed_before, "case")
            || ends_with_keyword(trimmed_before, "return")
            || ends_with_keyword(trimmed_before, "when")
            || ends_with_keyword(trimmed_before, "in")
        {
            return true;
        }

        // Check if the block is the last expression before `end` or `}`
        // (i.e., the return value of a do..end block or method body)
        // but NOT at the top level of the file
        if trimmed_after.starts_with("end") || trimmed_after.starts_with('}') {
            // Make sure this is inside an actual scope (not top level)
            // by checking that there's a corresponding `do`/`def`/`{` in the preceding context
            if self.inside_block_or_method_scope(call_start) {
                return true;
            }
        }

        false
    }

    /// Check if the given position is inside a block/method/class body (not at top level)
    fn inside_block_or_method_scope(&self, offset: usize) -> bool {
        let before = &self.ctx.source[..offset];
        // Simple heuristic: count unmatched do/def/class/module keywords vs end keywords
        // More reliable: check indentation level > 0
        let trimmed = before.trim();
        if trimmed.is_empty() {
            return false;
        }
        // Check if the current indentation suggests we're nested
        let line_start = self.ctx.line_start(offset);
        let indent = self.ctx.indentation_of(line_start);
        indent > 0
    }

    fn outermost_call_start(&self, call: &ruby_prism::CallNode) -> usize {
        if let Some(recv) = call.receiver() {
            return recv.location().start_offset();
        }
        call.location().start_offset()
    }

    fn message(
        &self,
        call: &ruby_prism::CallNode,
        block: &ruby_prism::BlockNode,
    ) -> String {
        let open_offset = block.opening_loc().start_offset();
        let close_offset = block.closing_loc().end_offset();
        let braces = self.is_braces(open_offset);
        let multiline = self.is_multiline(open_offset, close_offset);

        match self.cop.enforced_style {
            EnforcedStyle::LineCountBased => {
                if multiline {
                    "Avoid using `{...}` for multi-line blocks.".to_string()
                } else {
                    "Prefer `{...}` over `do...end` for single-line blocks.".to_string()
                }
            }
            EnforcedStyle::Semantic => {
                if braces {
                    "Prefer `do...end` over `{...}` for procedural blocks.".to_string()
                } else {
                    "Prefer `{...}` over `do...end` for functional blocks.".to_string()
                }
            }
            EnforcedStyle::BracesForChaining => {
                if multiline {
                    let chained = self.is_chained(call, block);
                    if chained {
                        "Prefer `{...}` over `do...end` for multi-line chained blocks."
                            .to_string()
                    } else {
                        "Prefer `do...end` for multi-line blocks without chaining."
                            .to_string()
                    }
                } else {
                    "Prefer `{...}` over `do...end` for single-line blocks.".to_string()
                }
            }
            EnforcedStyle::AlwaysBraces => {
                "Prefer `{...}` over `do...end` for blocks.".to_string()
            }
        }
    }

    fn add_offense(&mut self, block: &ruby_prism::BlockNode, message: &str) {
        let open_loc = block.opening_loc();
        let block_start = block.location().start_offset();
        let block_end = block.closing_loc().end_offset();

        self.offenses.push(self.ctx.offense_with_range(
            "Style/BlockDelimiters",
            message,
            Severity::Convention,
            open_loc.start_offset(),
            open_loc.end_offset(),
        ));

        // Mark this block range so descendant blocks are skipped
        self.offended_ranges.push((block_start, block_end));
    }
}

impl Visit<'_> for BlockVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Phase 1: Collect ignored blocks (arguments to unparenthesized calls)
        self.collect_ignored_from_call(node);

        // Phase 2: Check blocks attached to this call
        if let Some(block_node) = node.block() {
            if let Some(block) = block_node.as_block_node() {
                self.check_block(node, &block);
            }
        }

        // Continue traversal (visits children including block body)
        ruby_prism::visit_call_node(self, node);
    }
}

/// Check if trimmed text ends with a keyword (preceded by non-alphanumeric or start of string)
fn ends_with_keyword(text: &str, keyword: &str) -> bool {
    if !text.ends_with(keyword) {
        return false;
    }
    let prefix_len = text.len() - keyword.len();
    if prefix_len == 0 {
        return true;
    }
    let prev = text.as_bytes()[prefix_len - 1];
    !prev.is_ascii_alphanumeric() && prev != b'_'
}

fn is_operator_method(name: &str) -> bool {
    matches!(
        name,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "**"
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "<=>"
            | "<<"
            | ">>"
            | "&"
            | "|"
            | "^"
            | "~"
            | "!"
            | "[]"
            | "[]="
            | "+@"
            | "-@"
    )
}

impl Cop for BlockDelimiters {
    fn name(&self) -> &'static str {
        "Style/BlockDelimiters"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = BlockVisitor::new(self, ctx);
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
