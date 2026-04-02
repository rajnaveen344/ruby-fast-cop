use crate::cops::{CheckContext, Cop};
use crate::helpers::source::*;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

/// Style for how block bodies should be indented when on a method chain
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignWithStyle {
    StartOfLine,
    RelativeToReceiver,
}

/// Style for how access modifier sections should be indented
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsistencyStyle {
    Normal,
    IndentedInternalMethods,
}

/// Style for end alignment in assignments
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndAlignStyle {
    Keyword,
    Variable,
    StartOfLine,
}

/// Style for def/end alignment with modifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefEndAlignStyle {
    StartOfLine,
    Def,
}

/// Style for indentation (tabs vs spaces)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Spaces,
    Tabs,
}

/// Style for access modifier indentation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessModifierStyle {
    Indent,
    Outdent,
}

pub struct IndentationWidth {
    width: usize,
    align_with: AlignWithStyle,
    consistency_style: ConsistencyStyle,
    end_align_style: EndAlignStyle,
    def_end_align_style: DefEndAlignStyle,
    indent_style: IndentStyle,
    access_modifier_style: AccessModifierStyle,
    allowed_patterns: Vec<String>,
}

impl IndentationWidth {
    pub fn new(width: usize) -> Self {
        Self {
            width,
            align_with: AlignWithStyle::StartOfLine,
            consistency_style: ConsistencyStyle::Normal,
            end_align_style: EndAlignStyle::Keyword,
            def_end_align_style: DefEndAlignStyle::StartOfLine,
            indent_style: IndentStyle::Spaces,
            access_modifier_style: AccessModifierStyle::Indent,
            allowed_patterns: Vec::new(),
        }
    }

    pub fn with_config(
        width: usize,
        align_with: AlignWithStyle,
        consistency_style: ConsistencyStyle,
    ) -> Self {
        Self {
            width,
            align_with,
            consistency_style,
            end_align_style: EndAlignStyle::Keyword,
            def_end_align_style: DefEndAlignStyle::StartOfLine,
            indent_style: IndentStyle::Spaces,
            access_modifier_style: AccessModifierStyle::Indent,
            allowed_patterns: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_full_config(
        width: usize,
        align_with: AlignWithStyle,
        consistency_style: ConsistencyStyle,
        end_align_style: EndAlignStyle,
        def_end_align_style: DefEndAlignStyle,
        indent_style: IndentStyle,
        access_modifier_style: AccessModifierStyle,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self {
            width,
            align_with,
            consistency_style,
            end_align_style,
            def_end_align_style,
            indent_style,
            access_modifier_style,
            allowed_patterns,
        }
    }
}

impl Cop for IndentationWidth {
    fn name(&self) -> &'static str {
        "Layout/IndentationWidth"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = IndentationWidthVisitor {
            ctx,
            width: self.width,
            align_with: self.align_with,
            consistency_style: self.consistency_style,
            end_align_style: self.end_align_style,
            def_end_align_style: self.def_end_align_style,
            indent_style: self.indent_style,
            access_modifier_style: self.access_modifier_style,
            allowed_patterns: &self.allowed_patterns,
            offenses: Vec::new(),
            ignored_def_offsets: Vec::new(),
            ignored_if_offsets: Vec::new(),
            ignored_while_offsets: Vec::new(),
            ignored_until_offsets: Vec::new(),
            current_call_dot_off: None,
            current_call_receiver_last_line: None,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct IndentationWidthVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    width: usize,
    align_with: AlignWithStyle,
    consistency_style: ConsistencyStyle,
    end_align_style: EndAlignStyle,
    def_end_align_style: DefEndAlignStyle,
    indent_style: IndentStyle,
    access_modifier_style: AccessModifierStyle,
    allowed_patterns: &'a [String],
    offenses: Vec<Offense>,
    /// DefNode offsets that have been handled by modifier+def and should be ignored
    ignored_def_offsets: Vec<usize>,
    /// IfNode offsets handled by assignment check
    ignored_if_offsets: Vec<usize>,
    /// WhileNode offsets handled by assignment check
    ignored_while_offsets: Vec<usize>,
    /// UntilNode offsets handled by assignment check
    ignored_until_offsets: Vec<usize>,
    /// Dot offset for the current call node's block (for relative_to_receiver)
    current_call_dot_off: Option<usize>,
    /// Receiver's last line for the current call node (for dot_on_new_line check)
    current_call_receiver_last_line: Option<u32>,
}

/// Get the indentation string (whitespace before first non-ws) at the line containing offset
fn line_indentation_str(source: &str, offset: usize) -> &str {
    let ls = line_start_offset(source, offset);
    let first_nw = first_non_ws_col(source, offset);
    let end = ls + first_nw as usize;
    if end > source.len() {
        return "";
    }
    &source[ls..end]
}

/// Check if a line uses tabs in its indentation
fn line_uses_tabs(source: &str, offset: usize) -> bool {
    line_indentation_str(source, offset).contains('\t')
}

/// Compute visual column considering tabs
fn visual_column(source: &str, offset: usize, tab_width: usize) -> u32 {
    let indent = line_indentation_str(source, offset);
    let tab_count = indent.chars().filter(|c| *c == '\t').count();
    let space_count = indent.chars().filter(|c| *c == ' ').count();
    (tab_count * tab_width + space_count) as u32
}


use crate::helpers::access_modifier::is_bare_access_modifier as is_standalone_access_modifier;

/// Check if the first thing on the body line IS the body node
fn body_starts_at_line_start(source: &str, body_offset: usize) -> bool {
    let body_col = col_at_offset(source, body_offset);
    let first_nw = first_non_ws_col(source, body_offset);
    body_col == first_nw
}

/// Check if a line matches any of the allowed patterns
fn matches_allowed_pattern(source: &str, offset: usize, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let ls = line_start_offset(source, offset);
    let le = source[ls..].find('\n').map_or(source.len(), |p| ls + p);
    let line = &source[ls..le];
    for pat in patterns {
        if let Ok(re) = regex::Regex::new(pat) {
            if re.is_match(line) {
                return true;
            }
        }
    }
    false
}

/// Node type info for call chain unwrapping
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChainNodeType {
    If,
    While,
    Until,
    Other,
}

struct ChainNodeInfo {
    node_type: ChainNodeType,
    offset: usize,
}

/// Unwrap a method call chain to find the first part's info
fn first_part_of_call_chain_info(node: &ruby_prism::Node) -> ChainNodeInfo {
    match node {
        ruby_prism::Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                first_part_of_call_chain_info(&receiver)
            } else {
                ChainNodeInfo {
                    node_type: ChainNodeType::Other,
                    offset: node.location().start_offset(),
                }
            }
        }
        ruby_prism::Node::IfNode { .. } => ChainNodeInfo {
            node_type: ChainNodeType::If,
            offset: node.location().start_offset(),
        },
        ruby_prism::Node::WhileNode { .. } => ChainNodeInfo {
            node_type: ChainNodeType::While,
            offset: node.location().start_offset(),
        },
        ruby_prism::Node::UntilNode { .. } => ChainNodeInfo {
            node_type: ChainNodeType::Until,
            offset: node.location().start_offset(),
        },
        _ => ChainNodeInfo {
            node_type: ChainNodeType::Other,
            offset: node.location().start_offset(),
        },
    }
}

/// Walk down a call chain and check the if node found at the bottom
fn check_assignment_if(
    visitor: &mut IndentationWidthVisitor,
    node: &ruby_prism::Node,
    base_off: usize,
) {
    match node {
        ruby_prism::Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                check_assignment_if(visitor, &receiver, base_off);
            }
        }
        ruby_prism::Node::IfNode { .. } => {
            let if_node = node.as_if_node().unwrap();
            visitor.check_if_chain(&if_node, base_off);
        }
        _ => {}
    }
}

fn check_assignment_loop(
    visitor: &mut IndentationWidthVisitor,
    node: &ruby_prism::Node,
    base_off: usize,
    target: ChainNodeType,
) {
    match node {
        ruby_prism::Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                check_assignment_loop(visitor, &receiver, base_off, target);
            }
        }
        ruby_prism::Node::WhileNode { .. } if matches!(target, ChainNodeType::While) => {
            let n = node.as_while_node().unwrap();
            visitor.check_loop(n.keyword_loc().start_offset(), n.closing_loc(), n.statements(), base_off);
        }
        ruby_prism::Node::UntilNode { .. } if matches!(target, ChainNodeType::Until) => {
            let n = node.as_until_node().unwrap();
            visitor.check_loop(n.keyword_loc().start_offset(), n.closing_loc(), n.statements(), base_off);
        }
        _ => {}
    }
}

/// Check if a node is an adjacent_def_modifier (call whose first argument is a def)
fn is_adjacent_def_modifier(call: &ruby_prism::CallNode) -> bool {
    if let Some(args) = call.arguments() {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() == 1 {
            return matches!(&arg_list[0], ruby_prism::Node::DefNode { .. });
        }
    }
    false
}

/// Check if a call chain contains a def modifier (e.g., `public foo def test`)
/// Returns true if a def is found deeper in the chain
fn contains_def_in_modifier_chain(call: &ruby_prism::CallNode) -> bool {
    if let Some(args) = call.arguments() {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() == 1 {
            match &arg_list[0] {
                ruby_prism::Node::DefNode { .. } => return true,
                ruby_prism::Node::CallNode { .. } => {
                    let inner_call = arg_list[0].as_call_node().unwrap();
                    return is_adjacent_def_modifier(&inner_call)
                        || contains_def_in_modifier_chain(&inner_call);
                }
                _ => {}
            }
        }
    }
    false
}

/// Find the DefNode in a modifier chain and return its def keyword offset
fn find_def_kw_offset_in_chain(call: &ruby_prism::CallNode) -> Option<usize> {
    if let Some(args) = call.arguments() {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() == 1 {
            match &arg_list[0] {
                ruby_prism::Node::DefNode { .. } => {
                    let def = arg_list[0].as_def_node().unwrap();
                    return Some(def.def_keyword_loc().start_offset());
                }
                ruby_prism::Node::CallNode { .. } => {
                    let inner = arg_list[0].as_call_node().unwrap();
                    return find_def_kw_offset_in_chain(&inner);
                }
                _ => {}
            }
        }
    }
    None
}

/// Get the first body statement offset from a Node that may be StatementsNode, BeginNode, etc.
fn first_body_offset(body: &ruby_prism::Node) -> Option<usize> {
    match body {
        ruby_prism::Node::StatementsNode { .. } => {
            let stmts = body.as_statements_node().unwrap();
            stmts
                .body()
                .iter()
                .next()
                .map(|n| n.location().start_offset())
        }
        ruby_prism::Node::BeginNode { .. } => {
            let begin = body.as_begin_node().unwrap();
            if let Some(stmts) = begin.statements() {
                stmts
                    .body()
                    .iter()
                    .next()
                    .map(|n| n.location().start_offset())
            } else {
                None
            }
        }
        _ => Some(body.location().start_offset()),
    }
}

impl<'a> IndentationWidthVisitor<'a> {
    fn using_tabs(&self) -> bool {
        self.indent_style == IndentStyle::Tabs
    }

    /// Column offset between body and base, handling tabs
    fn column_offset(&self, source: &str, body_off: usize, base_off: usize) -> i32 {
        if self.using_tabs() {
            let body_tabs = line_uses_tabs(source, body_off);
            let base_tabs = line_uses_tabs(source, base_off);
            if body_tabs || base_tabs {
                let bv = visual_column(source, body_off, self.width) as i32;
                let rv = visual_column(source, base_off, self.width) as i32;
                return bv - rv;
            }
        }
        let body_col = col_at_offset(source, body_off) as i32;
        let base_col = col_at_offset(source, base_off) as i32;
        body_col - base_col
    }

    fn first_statement_offset(stmts: &ruby_prism::StatementsNode) -> Option<usize> {
        stmts
            .body()
            .iter()
            .next()
            .map(|n| n.location().start_offset())
    }

    /// Core indentation check. `base_off` is the byte offset of the base keyword/location.
    /// `body_off` is the byte offset of the first body statement.
    fn check_indentation(&mut self, base_off: usize, body_off: usize, qualifier: Option<&str>) {
        let source = self.ctx.source;

        // Skip if body doesn't start at beginning of its line
        if !body_starts_at_line_start(source, body_off) {
            return;
        }

        // Skip if base and body are on the same line
        if line_at_offset(source, base_off) == line_at_offset(source, body_off) {
            return;
        }

        // Skip if base line matches AllowedPatterns
        if matches_allowed_pattern(source, base_off, self.allowed_patterns) {
            return;
        }

        // Skip if body starts with a bare access modifier (checked separately)
        if self.body_starts_with_access_modifier(body_off) {
            return;
        }

        let indentation = self.column_offset(source, body_off, base_off);
        let configured = self.width as i32;
        let delta = configured - indentation;

        if delta == 0 {
            return;
        }

        self.report_offense(source, body_off, indentation, qualifier);
    }

    /// Check if the body node is a begin node whose first child is an access modifier
    fn body_starts_with_access_modifier(&self, body_off: usize) -> bool {
        // This is a simplified check - we can't easily determine this from just offsets
        // The actual check happens in check_class_body via select_check_member
        false
    }

    /// Report an indentation offense
    fn report_offense(
        &mut self,
        source: &str,
        body_off: usize,
        indentation: i32,
        qualifier: Option<&str>,
    ) {
        let body_ls = line_start_offset(source, body_off);

        let (msg, start_off, end_off) = if self.using_tabs() {
            // Tab-based message
            let configured_tabs: i32 = 1;
            let actual_tabs = if self.width > 0 {
                indentation / self.width as i32
            } else {
                0
            };
            let msg = if let Some(q) = qualifier {
                format!(
                    "Use {} (not {}) tabs for{} indentation.",
                    configured_tabs,
                    actual_tabs,
                    format!(" {}", q)
                )
            } else {
                format!(
                    "Use {} (not {}) tabs for indentation.",
                    configured_tabs, actual_tabs
                )
            };
            // Offense range: the indentation characters
            let indent_str = line_indentation_str(source, body_off);
            let indent_len = indent_str.len();
            let body_start = body_ls + indent_len;
            // offending_range for tabs: begin_pos - line_indentation.length
            let ind = body_start - indent_len;
            if indentation >= 0 {
                (msg, ind, body_start)
            } else {
                (msg, body_start, ind)
            }
        } else {
            // Space-based message
            let msg = if let Some(q) = qualifier {
                format!(
                    "Use {} (not {}) spaces for {} indentation.",
                    self.width, indentation, q
                )
            } else {
                format!(
                    "Use {} (not {}) spaces for indentation.",
                    self.width, indentation
                )
            };
            // Offense range matches RuboCop's offending_range:
            // ind = begin_pos - indentation
            // if indentation >= 0: range = ind..begin_pos
            // else: range = begin_pos..ind
            let body_col = col_at_offset(source, body_off) as usize;
            let body_start = body_ls + body_col;
            let ind_offset = if indentation >= 0 {
                body_start.saturating_sub(indentation as usize)
            } else {
                body_start + (-indentation) as usize
            };
            if indentation >= 0 {
                (msg, ind_offset, body_start)
            } else {
                (msg, body_start, ind_offset)
            }
        };

        // Ensure valid range that doesn't cross line boundaries
        let start = start_off.min(source.len());
        // Cap end to end of line (don't cross into next line)
        let line_end = source[start..]
            .find('\n')
            .map_or(source.len(), |p| start + p);
        let end = end_off.min(line_end).min(source.len()).max(start + 1);

        let location = crate::offense::Location::from_offsets(source, start, end);
        self.offenses.push(Offense::new(
            "Layout/IndentationWidth",
            &msg,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    /// Check if/elsif/else chains
    fn check_if_chain(&mut self, node: &ruby_prism::IfNode, base_off: usize) {
        let source = self.ctx.source;

        if let Some(kw_loc) = node.if_keyword_loc() {
            let kw_text = std::str::from_utf8(kw_loc.as_slice()).unwrap_or("if");
            if kw_text != "if" {
                return;
            }

            // Modifier if (e.g., `foo if bar`) has no end keyword
            if node.end_keyword_loc().is_none() {
                return;
            }

            if let Some(end_loc) = node.end_keyword_loc() {
                let kw_line = line_at_offset(source, kw_loc.start_offset());
                let end_line = line_at_offset(source, end_loc.start_offset());
                if kw_line == end_line {
                    return;
                }
            }

            // Check body
            if let Some(stmts) = node.statements() {
                if let Some(first_off) = Self::first_statement_offset(&stmts) {
                    self.check_indentation(base_off, first_off, None);
                }
            }

            // Check elsif/else chains
            self.check_if_consequent(node);
        }
    }

    fn check_if_consequent(&mut self, node: &ruby_prism::IfNode) {
        if let Some(subsequent) = node.subsequent() {
            match &subsequent {
                ruby_prism::Node::IfNode { .. } => {
                    let elsif_node = subsequent.as_if_node().unwrap();
                    if let Some(kw_loc) = elsif_node.if_keyword_loc() {
                        let kw_off = kw_loc.start_offset();
                        if let Some(stmts) = elsif_node.statements() {
                            if let Some(first_off) = Self::first_statement_offset(&stmts) {
                                self.check_indentation(kw_off, first_off, None);
                            }
                        }
                        self.check_if_consequent(&elsif_node);
                    }
                }
                ruby_prism::Node::ElseNode { .. } => {
                    let else_node = subsequent.as_else_node().unwrap();
                    let kw_off = else_node.else_keyword_loc().start_offset();
                    if let Some(stmts) = else_node.statements() {
                        if let Some(first_off) = Self::first_statement_offset(&stmts) {
                            self.check_indentation(kw_off, first_off, None);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn check_case_when(&mut self, node: &ruby_prism::CaseNode) {
        for condition in node.conditions().iter() {
            if let ruby_prism::Node::WhenNode { .. } = &condition {
                let when_node = condition.as_when_node().unwrap();
                let kw_off = when_node.keyword_loc().start_offset();
                if let Some(stmts) = when_node.statements() {
                    if let Some(first_off) = Self::first_statement_offset(&stmts) {
                        self.check_indentation(kw_off, first_off, None);
                    }
                }
            }
        }

        // Check else
        if let Some(else_node) = node.else_clause() {
            let kw_off = else_node.else_keyword_loc().start_offset();
            if let Some(stmts) = else_node.statements() {
                if let Some(first_off) = Self::first_statement_offset(&stmts) {
                    self.check_indentation(kw_off, first_off, None);
                }
            }
        }
    }

    fn check_case_match(&mut self, node: &ruby_prism::CaseMatchNode) {
        for condition in node.conditions().iter() {
            if let ruby_prism::Node::InNode { .. } = &condition {
                let in_node = condition.as_in_node().unwrap();
                let kw_off = in_node.in_loc().start_offset();
                if let Some(stmts) = in_node.statements() {
                    if let Some(first_off) = Self::first_statement_offset(&stmts) {
                        self.check_indentation(kw_off, first_off, None);
                    }
                }
            }
        }

        if let Some(else_node) = node.else_clause() {
            let kw_off = else_node.else_keyword_loc().start_offset();
            if let Some(stmts) = else_node.statements() {
                if let Some(first_off) = Self::first_statement_offset(&stmts) {
                    self.check_indentation(kw_off, first_off, None);
                }
            }
        }
    }

    fn check_block(&mut self, node: &ruby_prism::BlockNode) {
        let source = self.ctx.source;
        let open_loc = node.opening_loc();
        let close_loc = node.closing_loc();
        let close_off = close_loc.start_offset();

        // Check if closing is on its own line (begins its line)
        if !body_starts_at_line_start(source, close_off) {
            return;
        }

        if let Some(body) = node.body() {
            // Get the actual first statement offset, handling BeginNode wrapping
            let actual_body_off = match &body {
                ruby_prism::Node::BeginNode { .. } => {
                    let begin = body.as_begin_node().unwrap();
                    if let Some(stmts) = begin.statements() {
                        Self::first_statement_offset(&stmts)
                    } else {
                        None
                    }
                }
                _ => Some(body.location().start_offset()),
            };

            if let Some(body_off) = actual_body_off {
                let open_off = open_loc.start_offset();
                let open_line = line_at_offset(source, open_off);
                let body_line = line_at_offset(source, body_off);

                if open_line == body_line {
                    // Body on same line as opening - skip
                } else {
                    // base = close location (end/}) position
                    let base_off = self.block_base_off(node, close_off);
                    self.check_indentation(base_off, body_off, None);
                }
            }

            // Handle rescue/ensure inside block body
            if let ruby_prism::Node::BeginNode { .. } = &body {
                let begin = body.as_begin_node().unwrap();
                self.check_rescue_ensure_bodies(&begin);
            }

            // Handle indented_internal_methods in blocks
            if self.consistency_style == ConsistencyStyle::IndentedInternalMethods {
                if let ruby_prism::Node::BeginNode { .. } = &body {
                    let begin = body.as_begin_node().unwrap();
                    if let Some(stmts) = begin.statements() {
                        let stmts_list: Vec<_> = stmts.body().iter().collect();
                        if self.contains_access_modifier(&stmts_list) {
                            self.check_indented_internal_methods(&stmts_list);
                        }
                    }
                } else if let ruby_prism::Node::StatementsNode { .. } = &body {
                    let stmts = body.as_statements_node().unwrap();
                    let stmts_list: Vec<_> = stmts.body().iter().collect();
                    if self.contains_access_modifier(&stmts_list) {
                        self.check_indented_internal_methods(&stmts_list);
                    }
                }
            }
        }
    }

    /// Get the base offset for block indentation
    fn block_base_off(&self, _node: &ruby_prism::BlockNode, close_off: usize) -> usize {
        match self.align_with {
            AlignWithStyle::RelativeToReceiver => {
                // If dot is on a different line from the receiver, use dot position
                if let (Some(dot_off), Some(receiver_last_line)) = (
                    self.current_call_dot_off,
                    self.current_call_receiver_last_line,
                ) {
                    let dot_line = line_at_offset(self.ctx.source, dot_off);
                    if receiver_last_line < dot_line {
                        return dot_off;
                    }
                }
                close_off
            }
            AlignWithStyle::StartOfLine => close_off,
        }
    }

    /// Shared logic for class/singleton_class/module visitors
    fn check_class_like_body(&mut self, kw_off: usize, end_off: usize, body: Option<ruby_prism::Node>) {
        let source = self.ctx.source;
        if line_at_offset(source, kw_off) == line_at_offset(source, end_off) {
            return;
        }

        if let Some(body) = body {
            match &body {
                ruby_prism::Node::StatementsNode { .. } => {
                    let stmts = body.as_statements_node().unwrap();
                    self.check_class_body(kw_off, Some(&stmts));
                }
                ruby_prism::Node::BeginNode { .. } => {
                    let begin = body.as_begin_node().unwrap();
                    if let Some(stmts) = begin.statements() {
                        self.check_class_body(kw_off, Some(&stmts));
                    }
                    self.check_rescue_ensure_bodies(&begin);
                }
                _ => {}
            }
        }
    }

    fn check_class_body(
        &mut self,
        kw_off: usize,
        body: Option<&ruby_prism::StatementsNode>,
    ) {
        let source = self.ctx.source;

        if let Some(stmts) = body {
            let stmts_list: Vec<_> = stmts.body().iter().collect();
            if stmts_list.is_empty() {
                return;
            }

            let first = &stmts_list[0];
            let first_off = first.location().start_offset();

            let kw_line = line_at_offset(source, kw_off);
            let first_line = line_at_offset(source, first_off);

            if kw_line == first_line {
                return;
            }

            // select_check_member: if first member is access modifier
            let check_member = self.select_check_member(&stmts_list);

            if let Some(member_off) = check_member {
                self.check_indentation(kw_off, member_off, None);
            }

            if self.consistency_style == ConsistencyStyle::IndentedInternalMethods {
                self.check_indented_internal_methods(&stmts_list);
            } else {
                self.check_normal_access_modifier_sections(&stmts_list, kw_off);
            }
        }
    }

    /// select_check_member: Returns the offset of the member to check, or None if skipped
    fn select_check_member(&self, stmts: &[ruby_prism::Node]) -> Option<usize> {
        if stmts.is_empty() {
            return None;
        }
        let first = &stmts[0];
        if let ruby_prism::Node::CallNode { .. } = first {
            let call = first.as_call_node().unwrap();
            if is_standalone_access_modifier(&call) {
                // If access modifier indentation is outdent, skip
                if self.access_modifier_style == AccessModifierStyle::Outdent {
                    return None;
                }
                return Some(first.location().start_offset());
            }
        }
        Some(first.location().start_offset())
    }

    /// In normal consistency style, check non-modifier members that come AFTER an access modifier
    /// against the class base. Members before the first access modifier are checked by check_indentation.
    fn check_normal_access_modifier_sections(
        &mut self,
        stmts: &[ruby_prism::Node],
        class_kw_off: usize,
    ) {
        // Only check members that come after an access modifier
        let mut seen_modifier = false;
        for node in stmts {
            if let ruby_prism::Node::CallNode { .. } = node {
                let call = node.as_call_node().unwrap();
                if is_standalone_access_modifier(&call) {
                    seen_modifier = true;
                    continue;
                }
            }
            if seen_modifier {
                let node_off = node.location().start_offset();
                self.check_indentation(class_kw_off, node_off, None);
            }
        }
    }

    fn check_indented_internal_methods(&mut self, stmts: &[ruby_prism::Node]) {
        let mut current_modifier_off: Option<usize> = None;
        let mut is_public_section = false;

        for node in stmts {
            if let ruby_prism::Node::CallNode { .. } = node {
                let call = node.as_call_node().unwrap();
                if is_standalone_access_modifier(&call) {
                    let name = String::from_utf8_lossy(call.name().as_slice());
                    if name == "module_function" {
                        current_modifier_off = None;
                        is_public_section = false;
                        continue;
                    }
                    if name == "public" {
                        // public is the default section - methods don't need extra indentation
                        is_public_section = true;
                        current_modifier_off = None;
                        continue;
                    }
                    // private, protected trigger indented_internal_methods
                    is_public_section = false;
                    current_modifier_off = Some(node.location().start_offset());
                    continue;
                }
            }

            if is_public_section {
                continue;
            }

            if let Some(mod_off) = current_modifier_off {
                let node_off = node.location().start_offset();
                self.check_indentation(mod_off, node_off, Some("indented_internal_methods"));
                // Only check the first member after each modifier
                current_modifier_off = None;
            }
        }
    }

    fn contains_access_modifier(&self, stmts: &[ruby_prism::Node]) -> bool {
        stmts.iter().any(|node| {
            if let ruby_prism::Node::CallNode { .. } = node {
                let call = node.as_call_node().unwrap();
                is_standalone_access_modifier(&call)
            } else {
                false
            }
        })
    }

    fn check_def(&mut self, node: &ruby_prism::DefNode) {
        let source = self.ctx.source;

        if node.end_keyword_loc().is_none() {
            return;
        }

        let def_kw_off = node.def_keyword_loc().start_offset();

        // Check if this def was already handled by modifier+def
        if self.ignored_def_offsets.contains(&def_kw_off) {
            return;
        }

        if let Some(end_loc) = node.end_keyword_loc() {
            let def_line = line_at_offset(source, def_kw_off);
            let end_line = line_at_offset(source, end_loc.start_offset());
            if def_line == end_line {
                return;
            }
        }

        let body = node.body();
        if let Some(ref b) = body {
            match b {
                ruby_prism::Node::BeginNode { .. } => {
                    let begin = b.as_begin_node().unwrap();
                    if let Some(stmts) = begin.statements() {
                        if let Some(first_off) = Self::first_statement_offset(&stmts) {
                            self.check_indentation(def_kw_off, first_off, None);
                        }
                    }
                    self.check_rescue_ensure_bodies(&begin);
                }
                _ => {
                    let bo = b.location().start_offset();
                    self.check_indentation(def_kw_off, bo, None);
                }
            }
        }
    }

    fn check_rescue_ensure_bodies(&mut self, begin_node: &ruby_prism::BeginNode) {
        if let Some(rescue) = begin_node.rescue_clause() {
            self.check_rescue_clause(&rescue);
        }

        if let Some(else_node) = begin_node.else_clause() {
            let kw_off = else_node.else_keyword_loc().start_offset();
            if let Some(stmts) = else_node.statements() {
                if let Some(first_off) = Self::first_statement_offset(&stmts) {
                    self.check_indentation(kw_off, first_off, None);
                }
            }
        }

        if let Some(ensure_node) = begin_node.ensure_clause() {
            let kw_off = ensure_node.ensure_keyword_loc().start_offset();
            if let Some(stmts) = ensure_node.statements() {
                if let Some(first_off) = Self::first_statement_offset(&stmts) {
                    self.check_indentation(kw_off, first_off, None);
                }
            }
        }
    }

    fn check_rescue_clause(&mut self, rescue: &ruby_prism::RescueNode) {
        let kw_off = rescue.keyword_loc().start_offset();
        if let Some(stmts) = rescue.statements() {
            if let Some(first_off) = Self::first_statement_offset(&stmts) {
                self.check_indentation(kw_off, first_off, None);
            }
        }
        if let Some(subsequent) = rescue.subsequent() {
            self.check_rescue_clause(&subsequent);
        }
    }

    /// Handle assignment nodes: check if RHS is if/while/until and apply alignment
    fn check_assignment_rhs(&mut self, lhs_off: usize, rhs: &ruby_prism::Node) {
        let source = self.ctx.source;

        // Unwrap method call chain to find the actual RHS info
        let rhs_info = first_part_of_call_chain_info(rhs);

        // variable_alignment?: if end_align_style == keyword -> false
        // Otherwise, check if rhs is on same line as lhs
        let variable_alignment = if self.end_align_style == EndAlignStyle::Keyword {
            false
        } else {
            let lhs_line = line_at_offset(source, lhs_off);
            let rhs_line = line_at_offset(source, rhs_info.offset);
            lhs_line == rhs_line
        };

        let base_off = if variable_alignment {
            lhs_off
        } else {
            rhs_info.offset
        };

        match rhs_info.node_type {
            ChainNodeType::If => {
                self.ignored_if_offsets.push(rhs_info.offset);
                // Use check_assignment_if to walk to the if node and check it
                check_assignment_if(self, rhs, base_off);
            }
            ChainNodeType::While => {
                self.ignored_while_offsets.push(rhs_info.offset);
                check_assignment_loop(self, rhs, base_off, ChainNodeType::While);
            }
            ChainNodeType::Until => {
                self.ignored_until_offsets.push(rhs_info.offset);
                check_assignment_loop(self, rhs, base_off, ChainNodeType::Until);
            }
            ChainNodeType::Other => {}
        }
    }

    /// Shared check for while/until loops
    fn check_loop(
        &mut self,
        kw_off: usize,
        closing_loc: Option<ruby_prism::Location>,
        statements: Option<ruby_prism::StatementsNode>,
        base_off: usize,
    ) {
        let closing = match closing_loc {
            Some(c) => c,
            None => return, // modifier form (no closing keyword)
        };

        let source = self.ctx.source;
        if line_at_offset(source, kw_off) == line_at_offset(source, closing.start_offset()) {
            return;
        }

        if let Some(stmts) = statements {
            if let Some(first_off) = Self::first_statement_offset(&stmts) {
                self.check_indentation(base_off, first_off, None);
            }
        }
    }

    /// Handle modifier+def pattern (e.g., `private def foo` or `foo def test` or `public foo def test`)
    fn check_modifier_def(&mut self, call_node: &ruby_prism::CallNode) {
        // Find the def keyword offset - either direct or nested
        let def_kw_off = if is_adjacent_def_modifier(call_node) {
            let args = call_node.arguments().unwrap();
            let arg_list: Vec<_> = args.arguments().iter().collect();
            let def_node = arg_list[0].as_def_node().unwrap();
            def_node.def_keyword_loc().start_offset()
        } else if contains_def_in_modifier_chain(call_node) {
            match find_def_kw_offset_in_chain(call_node) {
                Some(off) => off,
                None => return,
            }
        } else {
            return;
        };

        // Check if this def was already handled (by an outer modifier)
        if self.ignored_def_offsets.contains(&def_kw_off) {
            return;
        }

        // Mark this def as ignored
        self.ignored_def_offsets.push(def_kw_off);

        let source = self.ctx.source;

        // We need to get DefNode info - find it again
        // Since we know it's either direct arg or nested, re-walk to get body info
        struct DefInfo {
            has_end: bool,
            is_multiline: bool,
            body_first_off: Option<usize>,
        }

        fn extract_def_info(call: &ruby_prism::CallNode, source: &str) -> Option<DefInfo> {
            if let Some(args) = call.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if arg_list.len() == 1 {
                    match &arg_list[0] {
                        ruby_prism::Node::DefNode { .. } => {
                            let def = arg_list[0].as_def_node().unwrap();
                            let has_end = def.end_keyword_loc().is_some();
                            let is_multiline = if let Some(end_loc) = def.end_keyword_loc() {
                                line_at_offset(source, def.def_keyword_loc().start_offset())
                                    != line_at_offset(source, end_loc.start_offset())
                            } else {
                                false
                            };
                            let body_first_off = if let Some(body) = def.body() {
                                first_body_offset(&body)
                            } else {
                                None
                            };
                            return Some(DefInfo {
                                has_end,
                                is_multiline,
                                body_first_off,
                            });
                        }
                        ruby_prism::Node::CallNode { .. } => {
                            let inner = arg_list[0].as_call_node().unwrap();
                            return extract_def_info(&inner, source);
                        }
                        _ => {}
                    }
                }
            }
            None
        }

        let def_info = match extract_def_info(call_node, source) {
            Some(info) => info,
            None => return,
        };

        if !def_info.has_end || !def_info.is_multiline {
            return;
        }

        // Determine base
        let base_off = match self.def_end_align_style {
            DefEndAlignStyle::Def => def_kw_off,
            DefEndAlignStyle::StartOfLine => call_node.location().start_offset(),
        };

        if let Some(body_off) = def_info.body_first_off {
            self.check_indentation(base_off, body_off, None);
        }

        // Handle rescue/ensure in def body
        // We need to re-walk to find the BeginNode
        fn check_def_rescue(visitor: &mut IndentationWidthVisitor, call: &ruby_prism::CallNode) {
            if let Some(args) = call.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if arg_list.len() == 1 {
                    match &arg_list[0] {
                        ruby_prism::Node::DefNode { .. } => {
                            let def = arg_list[0].as_def_node().unwrap();
                            if let Some(body) = def.body() {
                                if let ruby_prism::Node::BeginNode { .. } = &body {
                                    let begin = body.as_begin_node().unwrap();
                                    visitor.check_rescue_ensure_bodies(&begin);
                                }
                            }
                        }
                        ruby_prism::Node::CallNode { .. } => {
                            let inner = arg_list[0].as_call_node().unwrap();
                            check_def_rescue(visitor, &inner);
                        }
                        _ => {}
                    }
                }
            }
        }
        check_def_rescue(self, call_node);
    }

    // extract_rhs_from_node removed - handled directly in visitor methods
}

/// Generate visit methods for assignment nodes that check RHS indentation.
/// All follow the same pattern: extract lhs offset + rhs value, check, visit children.
macro_rules! impl_assignment_visit {
    ($method:ident, $node_ty:ident, $visit_fn:path) => {
        fn $method(&mut self, node: &ruby_prism::$node_ty) {
            let lhs_off = node.location().start_offset();
            let rhs = node.value();
            self.check_assignment_rhs(lhs_off, &rhs);
            $visit_fn(self, node);
        }
    };
}

impl Visit<'_> for IndentationWidthVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(kw_loc) = node.if_keyword_loc() {
            let kw_text = std::str::from_utf8(kw_loc.as_slice()).unwrap_or("if");
            if kw_text == "if" {
                let kw_off = kw_loc.start_offset();
                if !self.ignored_if_offsets.contains(&kw_off) {
                    self.check_if_chain(node, kw_off);
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let source = self.ctx.source;
        let kw_off = node.keyword_loc().start_offset();

        // Modifier unless has no end keyword
        if node.end_keyword_loc().is_none() {
            ruby_prism::visit_unless_node(self, node);
            return;
        }

        if let Some(end_loc) = node.end_keyword_loc() {
            let kw_line = line_at_offset(source, kw_off);
            let end_line = line_at_offset(source, end_loc.start_offset());
            if kw_line == end_line {
                ruby_prism::visit_unless_node(self, node);
                return;
            }
        }

        if let Some(stmts) = node.statements() {
            if let Some(first_off) = Self::first_statement_offset(&stmts) {
                self.check_indentation(kw_off, first_off, None);
            }
        }

        if let Some(else_node) = node.else_clause() {
            let else_kw_off = else_node.else_keyword_loc().start_offset();
            if let Some(stmts) = else_node.statements() {
                if let Some(first_off) = Self::first_statement_offset(&stmts) {
                    self.check_indentation(else_kw_off, first_off, None);
                }
            }
        }

        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let kw_off = node.keyword_loc().start_offset();
        if !self.ignored_while_offsets.contains(&kw_off) {
            self.check_loop(kw_off, node.closing_loc(), node.statements(), kw_off);
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let kw_off = node.keyword_loc().start_offset();
        if !self.ignored_until_offsets.contains(&kw_off) {
            self.check_loop(kw_off, node.closing_loc(), node.statements(), kw_off);
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        let source = self.ctx.source;
        let kw_off = node.for_keyword_loc().start_offset();
        let end_off = node.end_keyword_loc().start_offset();

        let kw_line = line_at_offset(source, kw_off);
        let end_line = line_at_offset(source, end_off);
        if kw_line != end_line {
            if let Some(stmts) = node.statements() {
                if let Some(first_off) = Self::first_statement_offset(&stmts) {
                    self.check_indentation(kw_off, first_off, None);
                }
            }
        }

        ruby_prism::visit_for_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.check_class_like_body(
            node.class_keyword_loc().start_offset(),
            node.end_keyword_loc().start_offset(),
            node.body(),
        );
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let kw_off = node.class_keyword_loc().start_offset();
        let end_off = node.end_keyword_loc().start_offset();

        if let Some(body) = node.body() {
            let kw_line = line_at_offset(self.ctx.source, kw_off);
            if kw_line == line_at_offset(self.ctx.source, body.location().start_offset()) {
                ruby_prism::visit_singleton_class_node(self, node);
                return;
            }
        }

        self.check_class_like_body(kw_off, end_off, node.body());
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.check_class_like_body(
            node.module_keyword_loc().start_offset(),
            node.end_keyword_loc().start_offset(),
            node.body(),
        );
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.check_def(node);
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        let source = self.ctx.source;

        if let Some(kw_loc) = node.begin_keyword_loc() {
            let kw_off = kw_loc.start_offset();
            if let Some(end_loc) = node.end_keyword_loc() {
                let end_off = end_loc.start_offset();
                let kw_line = line_at_offset(source, kw_off);
                let end_line = line_at_offset(source, end_off);

                // Only check if end keyword begins its line (RuboCop: begins_its_line?)
                if kw_line != end_line && body_starts_at_line_start(source, end_off) {
                    // For begin..end, use end keyword's position as base
                    if let Some(stmts) = node.statements() {
                        if let Some(first_off) = Self::first_statement_offset(&stmts) {
                            self.check_indentation(end_off, first_off, None);
                        }
                    }
                }
            }

            self.check_rescue_ensure_bodies(node);
        }

        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.check_block(node);
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        self.check_case_when(node);
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        self.check_case_match(node);
        ruby_prism::visit_case_match_node(self, node);
    }

    // Handle call nodes for:
    // 1. modifier+def pattern
    // 2. setter assignment (foo.bar = if ..., foo&.bar = if ...)
    // 3. set dot info for relative_to_receiver block indentation
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check modifier+def pattern
        self.check_modifier_def(node);

        // Check setter assignment (on_send for setter methods)
        let name = String::from_utf8_lossy(node.name().as_slice());
        if name.ends_with('=')
            && !name.starts_with('!')
            && !name.starts_with('=')
            && name != "=="
            && name != "!="
            && name != "<="
            && name != ">="
        {
            // This is a setter call like foo.bar = ...
            if let Some(args) = node.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let Some(last_arg) = arg_list.last() {
                    let lhs_off = node.location().start_offset();
                    self.check_assignment_rhs(lhs_off, last_arg);
                }
            }
        }

        // Set dot info for blocks (relative_to_receiver)
        let prev_dot_off = self.current_call_dot_off;
        let prev_receiver_last_line = self.current_call_receiver_last_line;
        if node.block().is_some() {
            if let Some(call_op_loc) = node.call_operator_loc() {
                self.current_call_dot_off = Some(call_op_loc.start_offset());
                if let Some(receiver) = node.receiver() {
                    self.current_call_receiver_last_line = Some(line_at_offset(
                        self.ctx.source,
                        receiver.location().end_offset().saturating_sub(1),
                    ));
                } else {
                    self.current_call_receiver_last_line = None;
                }
            } else {
                self.current_call_dot_off = None;
                self.current_call_receiver_last_line = None;
            }
        }

        ruby_prism::visit_call_node(self, node);

        // Restore previous values
        self.current_call_dot_off = prev_dot_off;
        self.current_call_receiver_last_line = prev_receiver_last_line;
    }

    // Assignment visitors — all identical pattern via macro
    impl_assignment_visit!(visit_local_variable_write_node, LocalVariableWriteNode, ruby_prism::visit_local_variable_write_node);
    impl_assignment_visit!(visit_instance_variable_write_node, InstanceVariableWriteNode, ruby_prism::visit_instance_variable_write_node);
    impl_assignment_visit!(visit_class_variable_write_node, ClassVariableWriteNode, ruby_prism::visit_class_variable_write_node);
    impl_assignment_visit!(visit_global_variable_write_node, GlobalVariableWriteNode, ruby_prism::visit_global_variable_write_node);
    impl_assignment_visit!(visit_constant_write_node, ConstantWriteNode, ruby_prism::visit_constant_write_node);
    impl_assignment_visit!(visit_constant_path_write_node, ConstantPathWriteNode, ruby_prism::visit_constant_path_write_node);
    impl_assignment_visit!(visit_multi_write_node, MultiWriteNode, ruby_prism::visit_multi_write_node);
    impl_assignment_visit!(visit_local_variable_operator_write_node, LocalVariableOperatorWriteNode, ruby_prism::visit_local_variable_operator_write_node);
    impl_assignment_visit!(visit_local_variable_or_write_node, LocalVariableOrWriteNode, ruby_prism::visit_local_variable_or_write_node);
    impl_assignment_visit!(visit_local_variable_and_write_node, LocalVariableAndWriteNode, ruby_prism::visit_local_variable_and_write_node);
    impl_assignment_visit!(visit_index_operator_write_node, IndexOperatorWriteNode, ruby_prism::visit_index_operator_write_node);
    impl_assignment_visit!(visit_call_operator_write_node, CallOperatorWriteNode, ruby_prism::visit_call_operator_write_node);
}
