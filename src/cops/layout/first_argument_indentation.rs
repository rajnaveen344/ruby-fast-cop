use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstArgumentIndentationStyle {
    SpecialForInnerMethodCallInParentheses,
    SpecialForInnerMethodCall,
    Consistent,
    ConsistentRelativeToReceiver,
}

pub struct FirstArgumentIndentation {
    style: FirstArgumentIndentationStyle,
    indentation_width: Option<usize>,
}

impl FirstArgumentIndentation {
    pub fn new(style: FirstArgumentIndentationStyle, indentation_width: Option<usize>) -> Self {
        Self {
            style,
            indentation_width,
        }
    }
}

impl Cop for FirstArgumentIndentation {
    fn name(&self) -> &'static str {
        "Layout/FirstArgumentIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            style: self.style,
            indentation_width: self.indentation_width.unwrap_or(2),
            offenses: Vec::new(),
            arg_parent: None,
            in_interpolation: false,
            splat_start: None,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Debug, Clone)]
struct ParentCallInfo {
    start_offset: usize,
    is_parenthesized: bool,
    is_eligible: bool,
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: FirstArgumentIndentationStyle,
    indentation_width: usize,
    offenses: Vec<Offense>,
    /// Set when we are visiting argument nodes of a call
    arg_parent: Option<ParentCallInfo>,
    in_interpolation: bool,
    /// If the current call is inside a splat/kwsplat, this is the splat's start offset
    splat_start: Option<usize>,
}

impl<'a> Visitor<'a> {
    fn check_send(
        &mut self,
        node_start: usize,
        first_arg_start: usize,
        first_arg_end_raw: usize,
        has_dot: bool,
        is_operator: bool,
        is_setter: bool,
    ) {
        if self.in_interpolation {
            return;
        }
        if is_setter {
            return;
        }
        if is_operator && !has_dot {
            return;
        }
        if self.ctx.same_line(node_start, first_arg_start) {
            return;
        }
        // The first arg must begin its line (be the first non-ws char)
        if !self.ctx.begins_its_line(first_arg_start) {
            return;
        }

        let parent_info = &self.arg_parent;
        let use_special = self.special_inner_call_indentation(node_start, parent_info);

        let base_indent = if use_special {
            self.column_of_base_range(node_start, first_arg_start)
        } else {
            let first_arg_line = self.ctx.line_of(first_arg_start);
            self.previous_code_line_indent(first_arg_line)
        };

        let expected = base_indent + self.indentation_width;
        let actual = self.ctx.col_of(first_arg_start);

        if actual == expected {
            return;
        }

        // Check if this offense is within a range that another offense is already covering
        // (RuboCop calls this "lines affected by another offense" -> "Bad indentation")
        let arg_line = self.ctx.line_of(first_arg_start) as u32;
        let is_within_existing = self.offenses.iter().any(|o| {
            // If the current arg is on a line after an existing offense
            // and within a few lines, it's "affected by another offense"
            arg_line > o.location.line
                && arg_line <= o.location.line + 5
                && (o.location.column as usize) < actual
        });

        let message = if is_within_existing {
            "Bad indentation of the first argument.".to_string()
        } else {
            self.build_message(node_start, first_arg_start, use_special)
        };

        // Offense spans the first line of the first argument only
        let first_arg_end = self.end_of_first_line(first_arg_start, first_arg_end_raw);
        let location =
            crate::offense::Location::from_offsets(self.ctx.source, first_arg_start, first_arg_end);
        self.offenses.push(Offense::new(
            "Layout/FirstArgumentIndentation",
            message,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    fn special_inner_call_indentation(
        &self,
        node_start: usize,
        parent_info: &Option<ParentCallInfo>,
    ) -> bool {
        use FirstArgumentIndentationStyle::*;
        match self.style {
            Consistent => false,
            ConsistentRelativeToReceiver => true,
            SpecialForInnerMethodCall | SpecialForInnerMethodCallInParentheses => {
                if let Some(pi) = parent_info {
                    if !pi.is_eligible {
                        return false;
                    }
                    if !pi.is_parenthesized
                        && self.style == SpecialForInnerMethodCallInParentheses
                    {
                        return false;
                    }
                    node_start > pi.start_offset
                } else {
                    false
                }
            }
        }
    }

    fn column_of_base_range(&self, send_start: usize, arg_start: usize) -> usize {
        let range_source = &self.ctx.source[send_start..arg_start];
        let stripped = range_source.trim();
        if stripped.contains('\n') {
            let extra_lines = stripped.chars().filter(|&c| c == '\n').count();
            self.previous_code_line_indent(
                self.ctx.line_of(send_start) + extra_lines + 1,
            )
        } else {
            // Use display_column for the send_start position
            self.display_col(send_start)
        }
    }

    fn build_message(
        &self,
        node_start: usize,
        first_arg_start: usize,
        use_special: bool,
    ) -> String {
        if use_special {
            let range_text = &self.ctx.source[node_start..first_arg_start];
            let stripped = range_text.trim();
            if !stripped.contains('\n') {
                return format!(
                    "Indent the first argument one step more than `{}`.",
                    stripped
                );
            }
        }

        let first_arg_line = self.ctx.line_of(first_arg_start);
        let prev_line = self.find_previous_code_line_number(first_arg_line);
        let has_intervening_comment = self.has_comment_between(prev_line, first_arg_line);

        if has_intervening_comment {
            "Indent the first argument one step more than the start of the previous line (not counting the comment).".to_string()
        } else {
            "Indent the first argument one step more than the start of the previous line."
                .to_string()
        }
    }

    /// Display column: counts fullwidth chars as 2 columns
    fn display_col(&self, offset: usize) -> usize {
        let ls = self.ctx.line_start(offset);
        let line_prefix = &self.ctx.source[ls..offset];
        display_width(line_prefix)
    }

    fn previous_code_line_indent(&self, line_number: usize) -> usize {
        let ln = self.find_previous_code_line_number(line_number);
        let text = self.get_line(ln);
        text.bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count()
    }

    fn find_previous_code_line_number(&self, line_number: usize) -> usize {
        let mut ln = line_number;
        loop {
            if ln <= 1 {
                return 1;
            }
            ln -= 1;
            let text = self.get_line(ln);
            let trimmed = text.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                return ln;
            }
        }
    }

    /// Compute end offset: min of `raw_end` and end-of-line from `start`.
    /// This makes the offense span only the first line of a multi-line expression.
    fn end_of_first_line(&self, start: usize, raw_end: usize) -> usize {
        let bytes = self.ctx.source.as_bytes();
        let mut i = start;
        while i < raw_end && i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        i
    }

    fn has_comment_between(&self, after_line: usize, before_line: usize) -> bool {
        let mut ln = after_line + 1;
        while ln < before_line {
            let text = self.get_line(ln);
            let trimmed = text.trim();
            if trimmed.starts_with('#') {
                return true;
            }
            ln += 1;
        }
        false
    }

    fn get_line(&self, line_number: usize) -> &str {
        if line_number == 0 {
            return "";
        }
        let mut current_line = 1usize;
        let bytes = self.ctx.source.as_bytes();
        let mut start = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                if current_line == line_number {
                    return &self.ctx.source[start..i];
                }
                current_line += 1;
                start = i + 1;
            }
        }
        if current_line == line_number {
            &self.ctx.source[start..]
        } else {
            ""
        }
    }

    /// Visit the arguments of a call, setting arg_parent for the duration
    fn visit_args_with_parent(
        &mut self,
        node: &ruby_prism::CallNode,
        parent_info: ParentCallInfo,
    ) {
        let old = self.arg_parent.take();
        self.arg_parent = Some(parent_info);
        // Visit arguments subtree only
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() {
                self.visit(&arg);
            }
        }
        // Visit block if present
        if let Some(block) = node.block() {
            self.visit(&block);
        }
        self.arg_parent = old;
    }

    fn visit_super_args_with_parent(
        &mut self,
        node: &ruby_prism::SuperNode,
    ) {
        // super doesn't set parent info (no method name to be eligible)
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() {
                self.visit(&arg);
            }
        }
        if let Some(block) = node.block() {
            self.visit(&block);
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name = node_name!(node).to_string();
        let is_setter = name.ends_with('=') && !name.ends_with("==") && name != "!=";
        let is_operator = is_operator_method(&name);
        let has_dot = node.call_operator_loc().is_some();
        let has_parens = node.opening_loc().is_some();

        // Check first argument indentation
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let Some(first_arg) = arg_list.first() {
                // If inside a splat, use the splat's start offset
                let effective_start = self.splat_start.unwrap_or_else(|| node.location().start_offset());
                self.check_send(
                    effective_start,
                    first_arg.location().start_offset(),
                    first_arg.location().end_offset(),
                    has_dot,
                    is_operator,
                    is_setter,
                );
            }
        }

        // Visit receiver without arg_parent (receiver is not an argument of this call)
        {
            let old_parent = self.arg_parent.take();
            if let Some(recv) = node.receiver() {
                self.visit(&recv);
            }
            self.arg_parent = old_parent;
        }

        // Visit arguments with this call as the parent
        let is_eligible = !name.ends_with("[]=") && !is_operator_method(&name);
        let parent_info = ParentCallInfo {
            start_offset: node.location().start_offset(),
            is_parenthesized: has_parens,
            is_eligible,
        };
        self.visit_args_with_parent(node, parent_info);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let Some(first_arg) = arg_list.first() {
                self.check_send(
                    node.location().start_offset(),
                    first_arg.location().start_offset(),
                    first_arg.location().end_offset(),
                    false,
                    false,
                    false,
                );
            }
        }
        self.visit_super_args_with_parent(node);
    }

    fn visit_splat_node(&mut self, node: &ruby_prism::SplatNode) {
        let old = self.splat_start.take();
        self.splat_start = Some(node.location().start_offset());
        ruby_prism::visit_splat_node(self, node);
        self.splat_start = old;
    }

    fn visit_assoc_splat_node(&mut self, node: &ruby_prism::AssocSplatNode) {
        let old = self.splat_start.take();
        self.splat_start = Some(node.location().start_offset());
        ruby_prism::visit_assoc_splat_node(self, node);
        self.splat_start = old;
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let old = self.in_interpolation;
        self.in_interpolation = true;
        ruby_prism::visit_interpolated_string_node(self, node);
        self.in_interpolation = old;
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        let old = self.in_interpolation;
        self.in_interpolation = true;
        ruby_prism::visit_interpolated_x_string_node(self, node);
        self.in_interpolation = old;
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        let old = self.in_interpolation;
        self.in_interpolation = true;
        ruby_prism::visit_interpolated_symbol_node(self, node);
        self.in_interpolation = old;
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        let old = self.in_interpolation;
        self.in_interpolation = true;
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
        self.in_interpolation = old;
    }
}

/// Simple display width: ASCII = 1, CJK fullwidth = 2
fn display_width(s: &str) -> usize {
    let mut w = 0;
    for ch in s.chars() {
        if is_fullwidth(ch) {
            w += 2;
        } else {
            w += 1;
        }
    }
    w
}

fn is_fullwidth(ch: char) -> bool {
    let cp = ch as u32;
    // CJK Fullwidth ranges (simplified)
    (0xFF01..=0xFF60).contains(&cp)
        || (0xFFE0..=0xFFE6).contains(&cp)
        || (0x3000..=0x303F).contains(&cp)
        || (0x4E00..=0x9FFF).contains(&cp)
        || (0x3040..=0x309F).contains(&cp)
        || (0x30A0..=0x30FF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        || (0x2E80..=0x2FFF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFE30..=0xFE4F).contains(&cp)
        || (0x1F000..=0x1F9FF).contains(&cp)
        || (0x20000..=0x2FA1F).contains(&cp)
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
            | ">"
            | "<"
            | ">="
            | "<="
            | "<=>"
            | "==="
            | "&"
            | "|"
            | "^"
            | "~"
            | "<<"
            | ">>"
            | "[]"
            | "[]="
            | "=~"
            | "!~"
            | "`"
    )
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }
impl Default for Cfg { fn default() -> Self { Self { enforced_style: "special_for_inner_method_call_in_parentheses".into() } } }

crate::register_cop!("Layout/FirstArgumentIndentation", |cfg| {
    let c: Cfg = cfg.typed("Layout/FirstArgumentIndentation");
    let style = match c.enforced_style.as_str() {
        "consistent" => FirstArgumentIndentationStyle::Consistent,
        "consistent_relative_to_receiver" => FirstArgumentIndentationStyle::ConsistentRelativeToReceiver,
        "special_for_inner_method_call" => FirstArgumentIndentationStyle::SpecialForInnerMethodCall,
        _ => FirstArgumentIndentationStyle::SpecialForInnerMethodCallInParentheses,
    };
    let width = cfg.get_cop_config("Layout/FirstArgumentIndentation")
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_i64())
        .map(|v| v as usize);
    Some(Box::new(FirstArgumentIndentation::new(style, width)))
});
