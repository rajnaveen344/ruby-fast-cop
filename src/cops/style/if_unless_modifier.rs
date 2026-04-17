//! Style/IfUnlessModifier - Checks for `if` and `unless` statements that would fit
//! on one line as modifier form, and modifier forms that make the line too long.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/if_unless_modifier.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/IfUnlessModifier";
const DEFAULT_MAX_LINE_LENGTH: usize = 80;

const MSG_USE_MODIFIER: &str = "Favor modifier `%KEYWORD%` usage when having a \
single-line body. Another good alternative is the usage of control flow `&&`/`||`.";
const MSG_USE_NORMAL: &str = "Modifier form of `%KEYWORD%` makes the line too long.";

pub struct IfUnlessModifier {
    max_line_length: usize,
    line_length_enabled: bool,
    allow_uri: bool,
    allow_cop_directives: bool,
    tab_indentation_width: Option<usize>,
}

impl Default for IfUnlessModifier {
    fn default() -> Self {
        Self {
            max_line_length: DEFAULT_MAX_LINE_LENGTH,
            line_length_enabled: true,
            allow_uri: true,
            allow_cop_directives: true,
            tab_indentation_width: None,
        }
    }
}

impl IfUnlessModifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        max_line_length: usize,
        line_length_enabled: bool,
        allow_uri: bool,
        allow_cop_directives: bool,
        tab_indentation_width: Option<usize>,
    ) -> Self {
        Self {
            max_line_length,
            line_length_enabled,
            allow_uri,
            allow_cop_directives,
            tab_indentation_width,
        }
    }
}

impl Cop for IfUnlessModifier {
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
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
            in_dstr: false,
            max_line_length: self.max_line_length,
            line_length_enabled: self.line_length_enabled,
            allow_uri: self.allow_uri,
            allow_cop_directives: self.allow_cop_directives,
            tab_indentation_width: self.tab_indentation_width,
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_dstr: bool,
    max_line_length: usize,
    line_length_enabled: bool,
    allow_uri: bool,
    allow_cop_directives: bool,
    tab_indentation_width: Option<usize>,
}

impl<'a> Visitor<'a> {
    fn check_node(
        &mut self,
        keyword: &str,
        keyword_start: usize,
        keyword_end: usize,
        predicate: &Node,
        statements: &Option<ruby_prism::StatementsNode>,
        has_elsif_or_else: bool,
        end_keyword_loc: Option<ruby_prism::Location>,
        node_start: usize,
        node_end: usize,
    ) {
        if self.in_dstr {
            return;
        }

        let is_modifier = end_keyword_loc.is_none();
        let body_items: Vec<Node> = statements
            .as_ref()
            .map(|s| s.body().iter().collect())
            .unwrap_or_default();

        // Skip endless method def in body
        if self.body_is_endless_def(&body_items) {
            return;
        }

        // Skip defined? with undefined argument
        if self.has_undefined_defined(predicate, node_start) {
            return;
        }

        // Skip pattern matching in condition
        if has_pattern_match(predicate) {
            return;
        }

        // --- MSG_USE_MODIFIER: multiline -> modifier ---
        if !is_modifier {
            if self.should_use_modifier(
                keyword, keyword_start, predicate, statements, &body_items,
                has_elsif_or_else, &end_keyword_loc, node_start, node_end,
            ) && !matches!(predicate, Node::MatchWriteNode { .. })
            {
                let msg = MSG_USE_MODIFIER.replace("%KEYWORD%", keyword);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME, &msg, Severity::Convention, keyword_start, keyword_end,
                ));
                return;
            }
        }

        // --- MSG_USE_NORMAL: modifier -> normal ---
        if is_modifier && !body_items.is_empty() {
            if self.too_long_due_to_modifier(node_start, node_end) {
                let msg = MSG_USE_NORMAL.replace("%KEYWORD%", keyword);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME, &msg, Severity::Convention, keyword_start, keyword_end,
                ));
            }
        }
    }

    // ── should_use_modifier ──

    fn should_use_modifier(
        &self,
        keyword: &str,
        keyword_start: usize,
        predicate: &Node,
        statements: &Option<ruby_prism::StatementsNode>,
        body_items: &[Node],
        has_elsif_or_else: bool,
        end_keyword_loc: &Option<ruby_prism::Location>,
        node_start: usize,
        node_end: usize,
    ) -> bool {
        // --- non_eligible_node? (IfUnlessModifier override) ---
        if has_elsif_or_else {
            return false;
        }

        // Chained: `.method`, `&.method`, `[`, or binary operator after `end`
        if self.is_chained_or_binary_after(node_end) {
            return false;
        }

        // Nested conditional (ternary in body)
        if self.body_has_nested_conditional(body_items) {
            return false;
        }

        // Multiline inside collection with shared lines
        if self.multiline_inside_collection(keyword_start, end_keyword_loc) {
            return false;
        }

        // --- non_eligible_node? (StatementModifier base) ---
        // Line count > 3 (non-empty lines)
        let node_src = &self.ctx.source[node_start..node_end];
        let non_empty_line_count = node_src.lines().filter(|l| !l.trim().is_empty()).count();
        if non_empty_line_count > 3 {
            return false;
        }

        // Comment on last line of node
        if let Some(end_loc) = end_keyword_loc {
            let last_line_num = self.ctx.line_of(end_loc.start_offset());
            if self.line_has_comment(last_line_num) {
                return false;
            }
        }

        // First-line comment + code after end
        let first_line_has_comment = self.line_has_comment(self.ctx.line_of(keyword_start));
        if first_line_has_comment && self.has_code_after_end(end_keyword_loc) {
            return false;
        }

        // --- non_eligible_body? ---
        if body_items.is_empty() {
            return false;
        }
        if body_items.len() > 1 {
            return false;
        }
        // Body has comments (check the body's source lines)
        if let Some(stmts) = statements {
            if self.region_contains_comment(stmts.location().start_offset(), stmts.location().end_offset()) {
                return false;
            }
        }

        // --- non_eligible_condition? ---
        if has_lvasgn_in_condition(predicate) {
            return false;
        }

        // modifier_fits_on_single_line?
        self.modifier_fits(keyword, keyword_start, predicate, body_items, end_keyword_loc)
    }

    fn modifier_fits(
        &self,
        keyword: &str,
        keyword_start: usize,
        predicate: &Node,
        body_items: &[Node],
        end_keyword_loc: &Option<ruby_prism::Location>,
    ) -> bool {
        if !self.line_length_enabled {
            return true;
        }
        let length = self.length_in_modifier_form(keyword, keyword_start, predicate, body_items, end_keyword_loc);
        length <= self.max_line_length
    }

    fn length_in_modifier_form(
        &self,
        keyword: &str,
        keyword_start: usize,
        predicate: &Node,
        body_items: &[Node],
        end_keyword_loc: &Option<ruby_prism::Location>,
    ) -> usize {
        if body_items.is_empty() {
            return 0;
        }

        let keyword_col = self.ctx.col_of(keyword_start);
        let line_text = self.ctx.line_text(keyword_start);
        let code_before = &line_text[..keyword_col.min(line_text.len())];

        let body = &body_items[0];
        let body_src = &self.ctx.source[body.location().start_offset()..body.location().end_offset()];
        let body_source = self.if_body_source(body_src, body);

        let cond_src = &self.ctx.source[predicate.location().start_offset()..predicate.location().end_offset()];
        let expression = format!("{} {} {}", body_source, keyword, cond_src);

        // Check if parenthesization is needed (assignment, operator, array, pair, call arg)
        let needs_parens = self.needs_parenthesization(keyword_start);
        let expression = if needs_parens {
            format!("({})", expression)
        } else {
            expression
        };

        // Check for first-line comment
        let first_line_comment = self.first_line_comment_text(keyword_start);
        let expression = match &first_line_comment {
            Some(c) => format!("{} {}", expression, c),
            None => expression,
        };

        // Check for code after end
        let code_after = self.code_after_end_str(end_keyword_loc);
        let full = match &code_after {
            Some(after) => format!("{}{}{}", code_before, expression, after),
            None => format!("{}{}", code_before, expression),
        };

        self.effective_line_length(&full)
    }

    /// Check if the if/unless needs to be wrapped in parens when converting to modifier form.
    /// This mirrors RuboCop's `parenthesize?` method which checks the parent node type.
    /// Since we don't have parent access, we scan backwards from the keyword.
    fn needs_parenthesization(&self, keyword_start: usize) -> bool {
        // Scan backwards through source (skipping whitespace including newlines)
        // to find the nearest significant token before the if/unless keyword
        let bytes = self.ctx.source.as_bytes();
        let mut i = keyword_start;
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b' ' | b'\t' | b'\n' | b'\r' => continue,
                // Assignment: `=` (but not `==`, `!=`, `>=`, `<=`)
                b'=' => {
                    if i > 0 && matches!(bytes[i - 1], b'=' | b'!' | b'>' | b'<') {
                        return false;
                    }
                    return true;
                }
                // Array element or hash brace
                b'[' | b'(' => return true,
                // After comma (array/hash/call)
                b',' => return true,
                // Hash pair value: `key:` (but not `::`)
                b':' => {
                    if i > 0 && bytes[i - 1] == b':' {
                        return false;
                    }
                    return true;
                }
                // Operators
                b'+' | b'-' | b'*' | b'/' | b'%' | b'^' => return true,
                // `||` or `&&`
                b'|' => return true,
                b'&' => return true,
                _ => return false,
            }
        }
        false
    }

    fn if_body_source(&self, body_source: &str, body_node: &Node) -> String {
        // Handle value omission in last hash arg of a bare call
        if let Some(call) = body_node.as_call_node() {
            let name = String::from_utf8_lossy(call.name().as_slice());
            if call.opening_loc().is_none() && name != "[]=" {
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if let Some(last) = arg_list.last() {
                        if self.has_value_omission_in_hash(last) {
                            return self.wrap_call_args(body_source, &call);
                        }
                    }
                }
            }
        }
        body_source.to_string()
    }

    fn has_value_omission_in_hash(&self, node: &Node) -> bool {
        if let Some(hash) = node.as_keyword_hash_node() {
            let elements: Vec<_> = hash.elements().iter().collect();
            if let Some(last) = elements.last() {
                if let Some(assoc) = last.as_assoc_node() {
                    let key_start = assoc.key().location().start_offset();
                    let key_end = assoc.key().location().end_offset();
                    let val_start = assoc.value().location().start_offset();
                    let val_end = assoc.value().location().end_offset();
                    let key_src = &self.ctx.source[key_start..key_end];
                    let val_src = &self.ctx.source[val_start..val_end];
                    if key_src.ends_with(':') {
                        let key_name = key_src.trim_end_matches(':');
                        if val_src == key_name {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn wrap_call_args(&self, body_source: &str, call: &ruby_prism::CallNode) -> String {
        let body_start = call.location().start_offset();
        if let Some(sel) = call.message_loc() {
            let method_end = sel.end_offset() - body_start;
            let method_src = &body_source[..method_end];
            if let Some(args) = call.arguments() {
                let args_start = args.location().start_offset() - body_start;
                let args_end = args.location().end_offset() - body_start;
                let args_src = &body_source[args_start..args_end];
                return format!("{}({})", method_src, args_src);
            }
        }
        body_source.to_string()
    }

    fn first_line_comment_text(&self, keyword_start: usize) -> Option<String> {
        let first_line = self.ctx.line_text(keyword_start);
        if let Some(hash_pos) = source::find_comment_start(first_line) {
            let comment = first_line[hash_pos..].trim_end();
            if !is_cop_directive(comment) {
                return Some(comment.to_string());
            }
        }
        None
    }

    fn code_after_end_str(&self, end_keyword_loc: &Option<ruby_prism::Location>) -> Option<String> {
        if let Some(end_loc) = end_keyword_loc {
            let end_line = self.ctx.line_text(end_loc.start_offset());
            let end_col = self.ctx.col_of(end_loc.start_offset());
            if end_col + 3 <= end_line.len() {
                let after = &end_line[end_col + 3..];
                if !after.trim().is_empty() {
                    return Some(after.to_string());
                }
            }
        }
        None
    }

    fn has_code_after_end(&self, end_keyword_loc: &Option<ruby_prism::Location>) -> bool {
        self.code_after_end_str(end_keyword_loc).is_some()
    }

    /// Effective line length accounting for tabs
    fn effective_line_length(&self, line: &str) -> usize {
        let tab_width = self.tab_indentation_width.unwrap_or(2);
        let mut len = 0;
        let mut at_start = true;
        for ch in line.chars() {
            if ch == '\t' && at_start {
                len += tab_width;
            } else {
                if ch != ' ' {
                    at_start = false;
                }
                len += 1;
            }
        }
        len
    }

    // ── too_long_due_to_modifier ──

    fn too_long_due_to_modifier(&self, node_start: usize, node_end: usize) -> bool {
        // Must be single line
        if self.ctx.source[node_start..node_end].contains('\n') {
            return false;
        }

        // Check if Layout/LineLength is disabled (via config or inline comments)
        if !self.line_length_enabled || self.line_length_disabled_at(node_start) {
            return false;
        }

        let line = self.ctx.line_text(node_start);
        let line_len = self.effective_line_length(line);
        if line_len <= self.max_line_length {
            return false;
        }

        // Check AllowURI
        if self.allow_uri && self.allowed_by_uri(line) {
            return false;
        }

        // Check AllowCopDirectives
        if self.allow_cop_directives && self.allowed_by_cop_directive(line) {
            return false;
        }

        // Check another_statement_on_same_line
        if self.another_statement_on_same_line(node_start, node_end) {
            return false;
        }

        true
    }

    fn line_length_disabled_at(&self, offset: usize) -> bool {
        let line = self.ctx.line_text(offset);
        if line.contains("rubocop:disable Layout/LineLength") {
            return true;
        }
        let line_num = self.ctx.line_of(offset);
        let mut disabled = false;
        for i in 1..line_num {
            let line_offset = source::line_byte_offset(self.ctx.source, i);
            let l = self.ctx.line_text(line_offset);
            if l.contains("rubocop:disable Layout/LineLength") {
                disabled = true;
            }
            if l.contains("rubocop:enable Layout/LineLength") {
                disabled = false;
            }
        }
        disabled
    }

    fn allowed_by_uri(&self, line: &str) -> bool {
        // Default AllowURI is true; find http(s) URLs
        let uri_start = line.find("http://").or_else(|| line.find("https://"));
        if let Some(start) = uri_start {
            // Find end of URI (whitespace or common delimiters)
            let rest = &line[start..];
            let uri_len = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
            let uri_end = start + uri_len;
            // URI makes line too long: starts before max and extends to end of line content
            if start < self.max_line_length && uri_end >= line.trim_end().len() {
                return true;
            }
        }
        false
    }

    fn allowed_by_cop_directive(&self, line: &str) -> bool {
        // Default AllowCopDirectives is true
        if let Some(pos) = line.find("# rubocop:") {
            let without = line[..pos].trim_end();
            if without.len() <= self.max_line_length {
                return true;
            }
        }
        false
    }

    fn another_statement_on_same_line(&self, node_start: usize, node_end: usize) -> bool {
        // Check if there's a semicolon followed by a real statement after the modifier if
        let line = self.ctx.line_text(node_start);
        let node_end_col = self.ctx.col_of(node_end);
        if node_end_col < line.len() {
            let after = &line[node_end_col..];
            // Only `;` followed by code counts as another statement
            // Closing braces/brackets/parens/`end` are NOT statements
            if let Some(semi_pos) = after.find(';') {
                let after_semi = after[semi_pos + 1..].trim();
                if !after_semi.is_empty() && !after_semi.starts_with('#') {
                    return true;
                }
            }
        }
        false
    }

    // ── Helpers ──

    fn body_is_endless_def(&self, body_items: &[Node]) -> bool {
        for item in body_items {
            if let Some(def) = item.as_def_node() {
                if def.end_keyword_loc().is_none() {
                    return true;
                }
            }
        }
        false
    }

    fn body_has_nested_conditional(&self, body_items: &[Node]) -> bool {
        for item in body_items {
            if let Some(if_node) = item.as_if_node() {
                if let Some(kw_loc) = if_node.if_keyword_loc() {
                    let kw = &self.ctx.source[kw_loc.start_offset()..kw_loc.end_offset()];
                    if kw != "if" && kw != "elsif" {
                        return true; // ternary
                    }
                } else {
                    return true; // no keyword = ternary
                }
            }
        }
        false
    }

    /// Check if a region of source contains comments on any line within it.
    /// This mirrors RuboCop's `processed_source.contains_comment?(body.source_range)`.
    /// We check each line that the body spans for `#` comments.
    fn region_contains_comment(&self, start: usize, end: usize) -> bool {
        // Get the line range
        let start_line = self.ctx.line_of(start);
        let end_line = self.ctx.line_of(end);
        for line_num in start_line..=end_line {
            let line_offset = source::line_byte_offset(self.ctx.source, line_num);
            let line_text = self.ctx.line_text(line_offset);
            if source::find_comment_start(line_text).is_some() {
                return true;
            }
        }
        false
    }

    /// Check if a specific line number has a comment
    fn line_has_comment(&self, line_num: usize) -> bool {
        let line_offset = source::line_byte_offset(self.ctx.source, line_num);
        let line_text = self.ctx.line_text(line_offset);
        source::find_comment_start(line_text).is_some()
    }

    fn has_undefined_defined(&self, condition: &Node, if_node_start: usize) -> bool {
        let defined_nodes = collect_defined_nodes(condition);
        for def_node in &defined_nodes {
            if self.defined_arg_is_undefined(def_node, if_node_start) {
                return true;
            }
        }
        false
    }

    fn defined_arg_is_undefined(&self, defined_node: &Node, if_node_start: usize) -> bool {
        let def = match defined_node.as_defined_node() {
            Some(d) => d,
            None => return false,
        };
        let arg = def.value();

        let var_name = match &arg {
            Node::LocalVariableReadNode { .. } => {
                let lvar = arg.as_local_variable_read_node().unwrap();
                String::from_utf8_lossy(lvar.name().as_slice()).to_string()
            }
            Node::CallNode { .. } => {
                let call = arg.as_call_node().unwrap();
                if call.receiver().is_some() {
                    return false;
                }
                if call.arguments().is_some() {
                    return false;
                }
                String::from_utf8_lossy(call.name().as_slice()).to_string()
            }
            _ => return false,
        };

        // Check for a preceding `var_name = ...` assignment (left sibling)
        // Look for `var_name = ` in source before the if node, respecting scope.
        // We use a simple heuristic: scan for `\nvar_name = ` or `^var_name = `
        let before_if = &self.ctx.source[..if_node_start];

        // Look for the variable being assigned (simple assignment form)
        // The pattern is: `var_name = ` at the start of a line or after newline
        for line in before_if.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(&var_name) {
                let rest = trimmed[var_name.len()..].trim_start();
                if rest.starts_with("= ") || rest.starts_with("=\n") || rest == "=" {
                    return false; // Found assignment, so it's defined
                }
            }
        }

        // Not found => undefined
        true
    }

    /// Check if `end` is followed by something that makes it non-eligible
    /// (binary operators, method calls, subscripts)
    fn is_chained_or_binary_after(&self, node_end: usize) -> bool {
        let bytes = self.ctx.source.as_bytes();
        let mut i = node_end;
        // Skip whitespace (including newlines for chaining)
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }
        if i >= bytes.len() {
            return false;
        }
        match bytes[i] {
            b'.' => true,
            b'&' => i + 1 < bytes.len() && bytes[i + 1] == b'.',
            b'[' => true,
            // Binary operators
            b'+' | b'-' | b'*' | b'/' | b'%' | b'|' | b'^' => true,
            _ => false,
        }
    }

    fn multiline_inside_collection(
        &self,
        keyword_start: usize,
        end_keyword_loc: &Option<ruby_prism::Location>,
    ) -> bool {
        if let Some(end_loc) = end_keyword_loc {
            let end_line = self.ctx.line_text(end_loc.start_offset());
            let end_col = self.ctx.col_of(end_loc.start_offset());

            // Check after `end` for another if/unless on the same line
            if end_col + 3 < end_line.len() {
                let after_end = &end_line[end_col + 3..];
                let trimmed = after_end.trim_start_matches(|c: char| {
                    c == ',' || c == ')' || c == ']' || c == '}' || c == ' ' || c == '\t'
                });
                if trimmed.starts_with("if ") || trimmed.starts_with("unless ")
                    || trimmed.starts_with("(if ") || trimmed.starts_with("(unless ")
                    || trimmed.starts_with("y:") || trimmed.starts_with("x:")
                {
                    // Check if it's truly another if/unless in a collection context
                    // More broadly: any non-empty content after `end,` / `end)` suggests
                    // sibling elements
                    let after_trimmed = after_end.trim();
                    if after_trimmed.starts_with(',') || after_trimmed.starts_with(")")
                        || after_trimmed.starts_with("]") || after_trimmed.starts_with("}")
                    {
                        // Look for if/unless after the separator
                        let rest = after_trimmed.trim_start_matches(|c: char| {
                            c == ',' || c == ' ' || c == '\t'
                        });
                        if rest.starts_with("if ") || rest.starts_with("unless ")
                            || rest.starts_with("(if ") || rest.starts_with("(unless ")
                        {
                            return true;
                        }
                        // Check for hash key: `y: if c`
                        if rest.contains(": if ") || rest.contains(": unless ")
                            || rest.contains(": (if ") || rest.contains(": (unless ")
                        {
                            return true;
                        }
                    }
                }
            }

            // More general: check if after `end` there's `, KEY: if/unless`
            if end_col + 3 < end_line.len() {
                let after_end = &end_line[end_col + 3..];
                let trimmed = after_end.trim();
                // Pattern: `, key: if/unless` or `), (if/unless`
                if let Some(comma_pos) = trimmed.find(',') {
                    let after_comma = trimmed[comma_pos + 1..].trim();
                    // Hash pair: `key: if ...`
                    if after_comma.contains(": if ") || after_comma.contains(": unless ")
                        || after_comma.contains(": (if ") || after_comma.contains(": (unless ")
                    {
                        return true;
                    }
                    // Direct if/unless
                    if after_comma.starts_with("if ") || after_comma.starts_with("unless ")
                        || after_comma.starts_with("(if ") || after_comma.starts_with("(unless ")
                    {
                        return true;
                    }
                }
            }

            // Check before our keyword for `end` from another if/unless on same line
            let kw_col = self.ctx.col_of(keyword_start);
            let kw_line = self.ctx.line_text(keyword_start);
            if kw_col > 0 {
                let before_kw = &kw_line[..kw_col];
                if before_kw.contains("end") {
                    return true;
                }
            }
        }
        false
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let was = self.in_dstr;
        self.in_dstr = true;
        ruby_prism::visit_interpolated_string_node(self, node);
        self.in_dstr = was;
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let keyword_loc = match node.if_keyword_loc() {
            Some(loc) => loc,
            None => {
                ruby_prism::visit_if_node(self, node);
                return;
            }
        };

        let kw_src = &self.ctx.source[keyword_loc.start_offset()..keyword_loc.end_offset()];
        if kw_src == "elsif" {
            ruby_prism::visit_if_node(self, node);
            return;
        }

        let has_elsif_or_else = if let Some(sub) = node.subsequent() {
            matches!(sub, Node::IfNode { .. } | Node::ElseNode { .. })
        } else {
            false
        };

        self.check_node(
            "if",
            keyword_loc.start_offset(),
            keyword_loc.end_offset(),
            &node.predicate(),
            &node.statements(),
            has_elsif_or_else,
            node.end_keyword_loc(),
            node.location().start_offset(),
            node.location().end_offset(),
        );

        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let keyword_loc = node.keyword_loc();
        let has_else = node.else_clause().is_some();

        self.check_node(
            "unless",
            keyword_loc.start_offset(),
            keyword_loc.end_offset(),
            &node.predicate(),
            &node.statements(),
            has_else,
            node.end_keyword_loc(),
            node.location().start_offset(),
            node.location().end_offset(),
        );

        ruby_prism::visit_unless_node(self, node);
    }
}

// ── Free functions ──

fn has_lvasgn_in_condition(node: &Node) -> bool {
    struct F { found: bool }
    impl Visit<'_> for F {
        fn visit_local_variable_write_node(&mut self, _: &ruby_prism::LocalVariableWriteNode) {
            self.found = true;
        }
        fn visit_local_variable_operator_write_node(&mut self, _: &ruby_prism::LocalVariableOperatorWriteNode) {
            self.found = true;
        }
        fn visit_local_variable_or_write_node(&mut self, _: &ruby_prism::LocalVariableOrWriteNode) {
            self.found = true;
        }
        fn visit_local_variable_and_write_node(&mut self, _: &ruby_prism::LocalVariableAndWriteNode) {
            self.found = true;
        }
    }
    let mut f = F { found: false };
    f.visit(node);
    f.found
}

fn has_pattern_match(node: &Node) -> bool {
    struct F { found: bool }
    impl Visit<'_> for F {
        fn visit_match_predicate_node(&mut self, _: &ruby_prism::MatchPredicateNode) {
            self.found = true;
        }
        fn visit_match_required_node(&mut self, _: &ruby_prism::MatchRequiredNode) {
            self.found = true;
        }
    }
    let mut f = F { found: false };
    f.visit(node);
    f.found
}

fn collect_defined_nodes<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    struct C<'b> { nodes: Vec<Node<'b>> }
    impl<'b> Visit<'b> for C<'b> {
        fn visit_defined_node(&mut self, node: &ruby_prism::DefinedNode<'b>) {
            self.nodes.push(node.as_node());
            ruby_prism::visit_defined_node(self, node);
        }
    }
    let mut c = C { nodes: Vec::new() };
    c.visit(node);
    c.nodes
}

fn is_cop_directive(comment: &str) -> bool {
    let normalized = comment.replace(' ', "");
    normalized.contains("rubocop:disable") || normalized.contains("rubocop:todo")
}

crate::register_cop!("Style/IfUnlessModifier", |cfg| {
    let ll_config = cfg.get_cop_config("Layout/LineLength");
    let ll_enabled = cfg.is_cop_enabled("Layout/LineLength");
    let max_ll = ll_config.and_then(|c| c.max).unwrap_or(80) as usize;
    let allow_uri = ll_config
        .and_then(|c| c.raw.get("AllowURI"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let allow_cop_directives = ll_config
        .and_then(|c| c.raw.get("AllowCopDirectives"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let tab_width = cfg.get_cop_config("Layout/IndentationStyle")
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_i64())
        .or_else(|| cfg.get_cop_config("Layout/IndentationWidth")
            .and_then(|c| c.raw.get("Width"))
            .and_then(|v| v.as_i64()))
        .map(|v| v as usize);
    Some(Box::new(IfUnlessModifier::with_config(
        max_ll, ll_enabled, allow_uri, allow_cop_directives, tab_width,
    )))
});
