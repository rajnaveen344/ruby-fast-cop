use crate::cops::{CheckContext, Cop};
use crate::helpers::source::find_comment_start as find_comment_in_line;
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::{Node, ProgramNode, Visit};

const MSG: &str = "Use safe navigation (`&.`) instead of checking if an object exists before calling the method.";
const COP_NAME: &str = "Style/SafeNavigation";

pub struct SafeNavigation {
    allowed_methods: Vec<String>,
    convert_code_that_can_start_to_return_nil: bool,
    max_chain_length: usize,
    safe_navigation_chain_enabled: bool,
}

impl SafeNavigation {
    pub fn new() -> Self {
        Self {
            allowed_methods: vec![
                "present?".into(), "blank?".into(), "presence".into(),
                "try".into(), "try!".into(),
            ],
            convert_code_that_can_start_to_return_nil: false,
            max_chain_length: 2,
            safe_navigation_chain_enabled: true,
        }
    }

    pub fn with_config(
        allowed_methods: Vec<String>,
        convert_code_that_can_start_to_return_nil: bool,
        max_chain_length: usize,
    ) -> Self {
        Self {
            allowed_methods,
            convert_code_that_can_start_to_return_nil,
            max_chain_length,
            safe_navigation_chain_enabled: true,
        }
    }

    pub fn with_full_config(
        allowed_methods: Vec<String>,
        convert_code_that_can_start_to_return_nil: bool,
        max_chain_length: usize,
        safe_navigation_chain_enabled: bool,
    ) -> Self {
        Self {
            allowed_methods,
            convert_code_that_can_start_to_return_nil,
            max_chain_length,
            safe_navigation_chain_enabled,
        }
    }
}

impl Cop for SafeNavigation {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 3) {
            return vec![];
        }
        let mut visitor = SafeNavVisitor {
            source: ctx.source,
            filename: ctx.filename,
            allowed_methods: &self.allowed_methods,
            convert_nil: self.convert_code_that_can_start_to_return_nil,
            max_chain_length: self.max_chain_length,
            safe_navigation_chain_enabled: self.safe_navigation_chain_enabled,
            offenses: Vec::new(),
        };
        visitor.visit_statements_node(&node.statements());
        visitor.offenses
    }
}

type Span = (usize, usize);

fn node_span(node: &Node) -> Span {
    let loc = node.location();
    (loc.start_offset(), loc.end_offset())
}

fn span_src<'a>(source: &'a str, s: Span) -> &'a str {
    &source[s.0..s.1]
}

fn node_src<'a>(source: &'a str, node: &Node) -> &'a str {
    span_src(source, node_span(node))
}

fn is_simple_var(node: &Node) -> bool {
    match node {
        Node::LocalVariableReadNode { .. }
        | Node::ConstantReadNode { .. }
        | Node::ConstantPathNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::GlobalVariableReadNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            call.is_variable_call() && call.receiver().is_none() && call.arguments().is_none()
        }
        _ => false,
    }
}

fn src_match_ignoring_safe_nav(a: &str, b: &str) -> bool {
    a == b || a.replace("&.", ".") == b.replace("&.", ".")
}

fn name_eq(name: &[u8], s: &str) -> bool {
    name == s.as_bytes()
}

fn has_dot(call: &ruby_prism::CallNode) -> bool {
    call.call_operator_loc().is_some()
}

fn is_double_colon(source: &str, call: &ruby_prism::CallNode) -> bool {
    call.call_operator_loc()
        .map_or(false, |loc| &source[loc.start_offset()..loc.end_offset()] == "::")
}

fn is_operator(name: &[u8]) -> bool {
    matches!(
        name,
        b"+" | b"-" | b"*" | b"/" | b"%" | b"**" | b"==" | b"!=" | b"<" | b">" | b"<=" | b">="
            | b"<=>" | b"<<" | b">>" | b"&" | b"|" | b"^" | b"~" | b"+@" | b"-@" | b"=~"
            | b"!~"
    )
}

fn is_comparison_or_arith(name: &[u8]) -> bool {
    matches!(
        name,
        b">" | b"<" | b">=" | b"<=" | b"==" | b"!=" | b"<=>" | b"=~" | b"!~" | b"+" | b"-"
            | b"*" | b"/" | b"%" | b"**" | b"<<" | b">>"
    )
}

fn end_of_first_line(source: &str, start_offset: usize) -> usize {
    source[start_offset..].find('\n').map_or(source.len(), |p| start_offset + p)
}

/// Try to extract a CallNode from a Node reference.
fn as_call<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    if matches!(node, Node::CallNode { .. }) {
        node.as_call_node()
    } else {
        None
    }
}

/// Get the method name of a call node, if the node is a CallNode.
fn call_name(node: &Node) -> Option<Vec<u8>> {
    as_call(node).map(|c| c.name().as_slice().to_vec())
}

struct SafeNavVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    allowed_methods: &'a [String],
    convert_nil: bool,
    max_chain_length: usize,
    safe_navigation_chain_enabled: bool,
    offenses: Vec<Offense>,
}

impl<'a> SafeNavVisitor<'a> {
    fn is_allowed_method(&self, node: &Node) -> bool {
        call_name(node).map_or(false, |n| {
            self.allowed_methods.iter().any(|m| m.as_bytes() == n.as_slice())
        })
    }

    fn ends_with_nil_check(&self, node: &Node) -> bool {
        call_name(node).map_or(false, |n| n == b"nil?")
    }

    fn ends_with_empty(&self, node: &Node) -> bool {
        call_name(node).map_or(false, |n| n == b"empty?")
    }

    fn is_negation(&self, node: &Node) -> bool {
        as_call(node).map_or(false, |c| name_eq(c.name().as_slice(), "!"))
    }

    fn is_logic_jump(&self, node: &Node) -> bool {
        matches!(node, Node::BreakNode{..} | Node::NextNode{..} | Node::ReturnNode{..} | Node::YieldNode{..})
        || as_call(node).map_or(false, |call| {
            call.receiver().is_none()
                && matches!(call.name().as_slice(), b"fail" | b"raise" | b"throw")
        })
    }

    fn is_assignment_call(&self, node: &Node) -> bool {
        as_call(node).map_or(false, |c| {
            let n = c.name().as_slice();
            n.ends_with(b"=") && n != b"==" && n != b"!="
        })
    }

    fn is_operator_call(&self, node: &Node) -> bool {
        as_call(node).map_or(false, |c| is_comparison_or_arith(c.name().as_slice()))
    }

    fn is_not_nil_check(&self, node: &Node) -> bool {
        as_call(node).map_or(false, |call| {
            name_eq(call.name().as_slice(), "!") && call.receiver().and_then(|recv| {
                as_call(&recv).filter(|inner| {
                    name_eq(inner.name().as_slice(), "nil?") && inner.receiver().is_some()
                })
            }).is_some()
        })
    }

    fn extract_checked_var_span(&self, cond: &Node) -> Option<(Span, bool)> {
        if is_simple_var(cond) {
            return Some((node_span(cond), false));
        }

        let call = as_call(cond)?;
        let name = call.name();

        if name_eq(name.as_slice(), "nil?") {
            return call.receiver().map(|recv| (node_span(&recv), true));
        }

        if name_eq(name.as_slice(), "!") {
            if let Some(recv) = call.receiver() {
                if let Some(inner) = as_call(&recv) {
                    if name_eq(inner.name().as_slice(), "nil?") {
                        if let Some(inner_recv) = inner.receiver() {
                            return Some((node_span(&inner_recv), true));
                        }
                    }
                }
                return Some((node_span(&recv), false));
            }
            return None;
        }

        Some((node_span(cond), false))
    }

    // ---- Unified chain analysis (works with any source context) ----

    fn find_matching_receiver(&self, chain: &Node, src: &str, var_src: &str) -> bool {
        if let Some(call) = as_call(chain) {
            if let Some(recv) = call.receiver() {
                if src_match_ignoring_safe_nav(node_src(src, &recv), var_src) {
                    return true;
                }
                return self.find_matching_receiver(&recv, src, var_src);
            }
        }
        false
    }

    fn chain_length(&self, node: &Node, src: &str, var_src: &str) -> usize {
        let mut count = 0;
        self.count_chain_recursive(node, src, var_src, &mut count);
        count
    }

    fn count_chain_recursive(&self, node: &Node, src: &str, var_src: &str, count: &mut usize) {
        if let Some(call) = as_call(node) {
            if let Some(recv) = call.receiver() {
                *count += 1;
                if src_match_ignoring_safe_nav(node_src(src, &recv), var_src) {
                    return;
                }
                self.count_chain_recursive(&recv, src, var_src, count);
            }
        }
    }

    fn chain_has_dotless_or_dcolon(&self, node: &Node, src: &str, var_src: &str) -> bool {
        if let Some(call) = as_call(node) {
            if !has_dot(&call) {
                let n = call.name();
                if name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") || is_operator(n.as_slice()) {
                    return true;
                }
            }
            if is_double_colon(src, &call) {
                return true;
            }
            if let Some(recv) = call.receiver() {
                if src_match_ignoring_safe_nav(node_src(src, &recv), var_src) {
                    return false;
                }
                return self.chain_has_dotless_or_dcolon(&recv, src, var_src);
            }
        }
        false
    }

    fn is_dotless_call_on_var(&self, body: &Node, src: &str, var_src: &str) -> bool {
        as_call(body).map_or(false, |call| {
            call.receiver().map_or(false, |recv| {
                src_match_ignoring_safe_nav(node_src(src, &recv), var_src) && !has_dot(&call)
            })
        })
    }

    fn has_unsafe_ancestor_in(&self, node: &Node, src: &str, var_src: &str) -> bool {
        if let Some(call) = as_call(node) {
            if let Some(recv) = call.receiver() {
                if src_match_ignoring_safe_nav(node_src(src, &recv), var_src) {
                    return false;
                }
                if !self.safe_navigation_chain_enabled {
                    return true;
                }
                if self.is_negation(&recv) {
                    return true;
                }
                return self.has_unsafe_ancestor_in(&recv, src, var_src);
            }
        }
        false
    }

    fn should_skip_body(&self, body: &Node, src: &str, var_src: &str) -> bool {
        self.is_logic_jump(body)
            || self.is_assignment_call(body)
            || self.ends_with_empty(body)
            || !self.find_matching_receiver(body, src, var_src)
            || self.chain_length(body, src, var_src) > self.max_chain_length
            || self.chain_has_dotless_or_dcolon(body, src, var_src)
            || self.is_operator_call(body)
            || self.is_dotless_call_on_var(body, src, var_src)
            || self.is_allowed_method(body)
            || self.ends_with_nil_check(body)
            || self.is_negation(body)
            || self.has_unsafe_ancestor_in(body, src, var_src)
    }

    // ---- If/Unless handling (unified) ----

    fn check_if_or_unless(&mut self, node_loc: ruby_prism::Location, condition: Node, statements: Option<ruby_prism::StatementsNode>, is_unless: bool) {
        let (checked_var_span, is_nil_form) = match self.extract_checked_var_span(&condition) {
            Some(v) => v,
            None => return,
        };

        if is_unless {
            if is_simple_var(&condition) { return; }
            let cond_src = node_src(self.source, &condition);
            if !is_nil_form && !cond_src.starts_with('!') { return; }
        }

        if !is_unless && !is_nil_form {
            let cond_src = node_src(self.source, &condition);
            if cond_src.starts_with('!') { return; }
        }

        let body_node = match statements {
            Some(stmts) => {
                let body = stmts.body();
                if body.len() != 1 { return; }
                body.iter().next().unwrap()
            }
            None => return,
        };

        let checked_var_src = span_src(self.source, checked_var_span);

        if self.should_skip_body(&body_node, self.source, checked_var_src) {
            return;
        }

        if let Some(call) = as_call(&body_node) {
            let _ = call;
            if let Some(method_call_node) = self.find_receiver_matching_var(&body_node, checked_var_src) {
                if self.is_dotless_operator_method(&method_call_node) {
                    return;
                }
                if let Some(mc) = as_call(&method_call_node) {
                    if is_double_colon(self.source, &mc) {
                        return;
                    }
                }
            }
        }

        let offense_end = end_of_first_line(self.source, node_loc.start_offset());
        let location = Location::from_offsets(self.source, node_loc.start_offset(), offense_end);
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        let body_src = node_src(self.source, &body_node);
        let corrected_body = self.add_safe_nav_all(body_src, checked_var_src);

        let full_node_src = &self.source[node_loc.start_offset()..node_loc.end_offset()];
        let corrected = self.build_if_correction(full_node_src, &corrected_body, &body_node, node_loc.start_offset());

        offense = offense.with_correction(Correction::replace(
            node_loc.start_offset(), node_loc.end_offset(), corrected,
        ));

        self.offenses.push(offense);
    }

    fn check_if(&mut self, node: &ruby_prism::IfNode) {
        let node_loc = node.location();
        let node_src_text = &self.source[node_loc.start_offset()..node_loc.end_offset()];

        if node_src_text.starts_with("elsif") { return; }

        let is_ternary = !node_src_text.starts_with("if")
            && !node_src_text.starts_with("unless")
            && node.subsequent().is_some()
            && node_src_text.contains('?');

        if is_ternary {
            self.check_ternary(node);
            return;
        }

        if node.subsequent().is_some() { return; }

        let is_unless = node_src_text.starts_with("unless");
        self.check_if_or_unless(node_loc, node.predicate(), node.statements(), is_unless);
    }

    fn check_unless(&mut self, node: &ruby_prism::UnlessNode) {
        if node.else_clause().is_some() { return; }
        self.check_if_or_unless(node.location(), node.predicate(), node.statements(), true);
    }

    fn find_receiver_matching_var<'b>(&self, node: &Node<'b>, checked_var_src: &str) -> Option<Node<'b>> {
        let call = as_call(node)?;
        if let Some(recv) = call.receiver() {
            if src_match_ignoring_safe_nav(node_src(self.source, &recv), checked_var_src) {
                return Some(call.as_node());
            }
            return self.find_receiver_matching_var(&recv, checked_var_src);
        }
        None
    }

    fn is_dotless_operator_method(&self, node: &Node) -> bool {
        as_call(node).map_or(false, |call| {
            call.call_operator_loc().is_none() && {
                let n = call.name();
                name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") || is_operator(n.as_slice())
            }
        })
    }

    fn build_if_correction(&self, if_src: &str, corrected_body: &str, body_node: &Node, if_start_offset: usize) -> String {
        let mut comments_before = Vec::new();
        let mut trailing_comment = String::new();

        let lines: Vec<&str> = if_src.lines().collect();
        let body_loc = body_node.location();
        let body_start_offset = body_loc.start_offset();
        let body_end_offset = body_loc.end_offset();

        for (i, line) in lines.iter().enumerate() {
            let line_trimmed = line.trim();

            if i == 0 {
                if let Some(hash_pos) = find_comment_in_line(line) {
                    let comment = line[hash_pos..].trim();
                    if !comment.is_empty() {
                        comments_before.push(comment.to_string());
                    }
                }
                continue;
            }

            if i == lines.len() - 1 {
                if let Some(hash_pos) = find_comment_in_line(line) {
                    trailing_comment = format!(" {}", line[hash_pos..].trim());
                }
                continue;
            }

            if line_trimmed.starts_with('#') {
                let line_offset = if_start_offset + if_src[..].lines().take(i).map(|l| l.len() + 1).sum::<usize>();
                if line_offset < body_start_offset || line_offset >= body_end_offset {
                    comments_before.push(line_trimmed.to_string());
                }
            }
        }

        let mut result = String::new();
        for comment in &comments_before {
            result.push_str(comment);
            result.push('\n');
        }
        result.push_str(corrected_body);
        result.push_str(&trailing_comment);
        result
    }

    // ---- Ternary handling ----

    fn check_ternary(&mut self, node: &ruby_prism::IfNode) {
        let condition = node.predicate();

        let then_node = node.statements().and_then(|stmts| {
            let body = stmts.body();
            if body.len() == 1 { body.iter().next() } else { None }
        });
        let else_node = node.subsequent().and_then(|sub| {
            if let Node::ElseNode { .. } = &sub {
                sub.as_else_node().unwrap().statements().and_then(|stmts| {
                    let body = stmts.body();
                    if body.len() == 1 { body.iter().next() } else { None }
                })
            } else { None }
        });

        let (then_node, else_node) = match (then_node, else_node) {
            (Some(t), Some(e)) => (t, e),
            _ => return,
        };

        let then_is_nil = matches!(then_node, Node::NilNode { .. });
        let else_is_nil = matches!(else_node, Node::NilNode { .. });

        if (!then_is_nil && !else_is_nil) || (then_is_nil && else_is_nil) { return; }

        let (checked_var_span, _) = if else_is_nil {
            let cond_src = node_src(self.source, &condition);
            match self.extract_checked_var_span(&condition) {
                Some((var_span, is_nil)) => {
                    if is_nil && !cond_src.starts_with('!') { return; }
                    if !is_nil && cond_src.starts_with('!') { return; }
                    (var_span, node_span(&then_node))
                }
                None => return,
            }
        } else {
            let cond_src = node_src(self.source, &condition);
            match self.extract_checked_var_span(&condition) {
                Some((var_span, is_nil)) => {
                    if !is_nil && !cond_src.starts_with('!') && is_simple_var(&condition) { return; }
                    if is_nil && cond_src.starts_with('!') { return; }
                    (var_span, node_span(&else_node))
                }
                None => return,
            }
        };

        let method_node = if else_is_nil { then_node } else { else_node };
        let checked_var_src = span_src(self.source, checked_var_span);

        if self.is_logic_jump(&method_node)
            || self.ends_with_empty(&method_node)
            || !self.find_matching_receiver(&method_node, self.source, checked_var_src)
            || self.chain_length(&method_node, self.source, checked_var_src) > self.max_chain_length
            || self.chain_has_dotless_or_dcolon(&method_node, self.source, checked_var_src)
            || self.is_allowed_method(&method_node)
            || self.ends_with_nil_check(&method_node)
            || self.is_negation(&method_node)
        { return; }

        if let Some(method_call) = self.find_receiver_matching_var(&method_node, checked_var_src) {
            if self.is_dotless_operator_method(&method_call) { return; }
            if let Some(mc) = as_call(&method_call) {
                if is_double_colon(self.source, &mc) { return; }
            }
        }

        let node_loc = node.location();
        let location = Location::from_offsets(self.source, node_loc.start_offset(), node_loc.end_offset());
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        let method_src = node_src(self.source, &method_node);
        let corrected = self.add_safe_nav_all(method_src, checked_var_src);
        offense = offense.with_correction(Correction::replace(
            node_loc.start_offset(), node_loc.end_offset(), corrected,
        ));

        self.offenses.push(offense);
    }

    // ---- AndNode handling ----

    fn check_and(&mut self, node: &ruby_prism::AndNode) {
        let and_node = node.as_node();
        let pairs = self.collect_and_clause_pairs(&and_node);

        let mut prev_rhs_end: Option<usize> = None;
        for (lhs_span, lhs_op_span, rhs_span) in &pairs {
            let is_overlap = prev_rhs_end.map_or(false, |end| lhs_span.0 < end);
            if self.check_and_pair(lhs_span, lhs_op_span, rhs_span, is_overlap) {
                prev_rhs_end = Some(rhs_span.1);
            }
        }
    }

    fn collect_and_clause_pairs(&self, node: &Node) -> Vec<(Span, Span, Span)> {
        let mut parts: Vec<(Span, bool)> = Vec::new();
        self.rubocop_and_parts(node, &mut parts);
        self.collect_descendant_and_parts(node, &mut parts, true);

        parts.sort_by_key(|&(span, _)| span.0);

        let mut slices: Vec<(Span, Option<Span>)> = Vec::new();
        let mut i = 0;
        while i < parts.len() {
            let (span1, is_op1) = parts[i];
            if i + 1 < parts.len() {
                let (span2, is_op2) = parts[i + 1];
                if !is_op1 && is_op2 {
                    slices.push((span1, Some(span2)));
                    i += 2;
                } else if !is_op1 {
                    slices.push((span1, None));
                    i += 1;
                } else {
                    i += 1;
                }
            } else {
                if !is_op1 { slices.push((span1, None)); }
                i += 1;
            }
        }

        let mut result = Vec::new();
        for j in 0..slices.len().saturating_sub(1) {
            if let Some(op_span) = slices[j].1 {
                result.push((slices[j].0, op_span, slices[j + 1].0));
            }
        }
        result
    }

    fn rubocop_and_parts(&self, node: &Node, parts: &mut Vec<(Span, bool)>) {
        if let Node::AndNode { .. } = node {
            let and = node.as_and_node().unwrap();
            let op_loc = and.operator_loc();
            parts.push(((op_loc.start_offset(), op_loc.end_offset()), true));

            let rhs = and.right();
            let lhs = and.left();

            if !self.and_inside_begin(&rhs) {
                parts.push((node_span(&rhs), false));
            }
            if !matches!(lhs, Node::AndNode { .. }) && !self.and_inside_begin(&lhs) {
                parts.push((node_span(&lhs), false));
            }
        }
    }

    fn collect_descendant_and_parts(&self, node: &Node, parts: &mut Vec<(Span, bool)>, is_top: bool) {
        match node {
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                if !is_top {
                    self.rubocop_and_parts(node, parts);
                }
                self.collect_descendant_and_parts(&and.left(), parts, false);
                self.collect_descendant_and_parts(&and.right(), parts, false);
            }
            Node::ParenthesesNode { .. } => {
                if let Some(body) = node.as_parentheses_node().unwrap().body() {
                    self.collect_descendant_and_parts(&body, parts, false);
                }
            }
            Node::StatementsNode { .. } => {
                for stmt in node.as_statements_node().unwrap().body().iter() {
                    self.collect_descendant_and_parts(&stmt, parts, false);
                }
            }
            Node::OrNode { .. } => {
                let or = node.as_or_node().unwrap();
                self.collect_descendant_and_parts(&or.left(), parts, false);
                self.collect_descendant_and_parts(&or.right(), parts, false);
            }
            _ => {}
        }
    }

    fn and_inside_begin(&self, node: &Node) -> bool {
        match node {
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if self.body_contains_and(&body) { return true; }
                    return self.and_inside_begin(&body);
                }
                false
            }
            Node::StatementsNode { .. } => {
                node.as_statements_node().unwrap().body().iter()
                    .any(|stmt| self.and_inside_begin(&stmt))
            }
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                self.and_inside_begin(&and.left()) || self.and_inside_begin(&and.right())
            }
            Node::OrNode { .. } => {
                let or = node.as_or_node().unwrap();
                self.and_inside_begin(&or.left()) || self.and_inside_begin(&or.right())
            }
            _ => false,
        }
    }

    fn body_contains_and(&self, node: &Node) -> bool {
        match node {
            Node::AndNode { .. } => true,
            Node::StatementsNode { .. } => {
                node.as_statements_node().unwrap().body().iter()
                    .any(|s| matches!(s, Node::AndNode { .. }))
            }
            _ => false,
        }
    }

    fn check_and_pair(&mut self, lhs_span: &Span, lhs_op_span: &Span, rhs_span: &Span, lhs_overlaps_prev_rhs: bool) -> bool {
        let lhs_src = span_src(self.source, *lhs_span);
        let rhs_src = span_src(self.source, *rhs_span);

        let lhs_parsed = ruby_prism::parse(lhs_src.as_bytes());
        let rhs_parsed = ruby_prism::parse(rhs_src.as_bytes());

        let lhs_node = {
            let prog = lhs_parsed.node();
            let prog = prog.as_program_node().unwrap();
            let body = prog.statements().body();
            if body.len() != 1 { return false; }
            body.iter().next().unwrap()
        };
        let rhs_node = {
            let prog = rhs_parsed.node();
            let prog = prog.as_program_node().unwrap();
            let body = prog.statements().body();
            if body.len() != 1 { return false; }
            body.iter().next().unwrap()
        };

        let is_not_nil = self.is_not_nil_check(&lhs_node);
        if is_not_nil && !self.convert_nil { return false; }

        let checked_var_src: String = if is_not_nil {
            match self.not_nil_receiver_src(lhs_src, &lhs_node) {
                Some(s) => s.to_string(),
                None => return false,
            }
        } else {
            match self.extract_and_lhs_src(lhs_src, &lhs_node) {
                Some(s) => s.to_string(),
                None => return false,
            }
        };

        if !is_not_nil && !self.is_valid_and_lhs(&lhs_node) { return false; }

        let actual_rhs = self.unwrap_parens_parsed(&rhs_node);
        if !self.find_matching_receiver(&actual_rhs, rhs_src, &checked_var_src) { return false; }

        if self.is_operator_call(&actual_rhs)
            || self.chain_length(&actual_rhs, rhs_src, &checked_var_src) > self.max_chain_length
            || self.chain_has_dotless_or_dcolon(&actual_rhs, rhs_src, &checked_var_src)
            || self.is_assignment_call(&actual_rhs)
            || self.is_allowed_method(&actual_rhs)
            || self.ends_with_nil_check(&actual_rhs)
            || self.is_negation(&actual_rhs)
            || self.ends_with_empty(&actual_rhs)
            || self.is_negation(&rhs_node)
            || self.has_unsafe_ancestor_in(&actual_rhs, rhs_src, &checked_var_src)
        { return false; }

        let location = Location::from_offsets(self.source, lhs_span.0, rhs_span.1);
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        if !self.node_contains_or(&rhs_node) {
            let edits = self.build_and_correction_edits(
                lhs_span, lhs_op_span, rhs_span,
                &checked_var_src, rhs_src, &actual_rhs,
                rhs_span.0, lhs_overlaps_prev_rhs,
            );
            if !edits.is_empty() {
                offense = offense.with_correction(Correction { edits });
            }
        }

        self.offenses.push(offense);
        true
    }

    fn build_and_correction_edits(
        &self, lhs_span: &Span, lhs_op_span: &Span, _rhs_span: &Span,
        checked_var_src: &str, rhs_src: &str, actual_rhs: &Node,
        rhs_base_offset: usize, lhs_overlaps_prev_rhs: bool,
    ) -> Vec<crate::offense::Edit> {
        use crate::offense::Edit;
        let mut edits = Vec::new();

        let mut dot_positions: Vec<usize> = Vec::new();
        self.collect_chain_dots(actual_rhs, rhs_src, checked_var_src, rhs_base_offset, &mut dot_positions);
        if dot_positions.is_empty() { return edits; }

        dot_positions.sort();
        let innermost_dot = dot_positions[0];

        if lhs_overlaps_prev_rhs {
            let op_start = self.skip_whitespace_left(lhs_op_span.0);
            let op_end = self.skip_whitespace_right(lhs_op_span.1);
            edits.push(Edit { start_offset: op_start, end_offset: op_end, replacement: String::new() });

            if let Some((recv_start, _)) = self.find_receiver_global_offset(actual_rhs, rhs_src, checked_var_src, rhs_base_offset) {
                edits.push(Edit { start_offset: recv_start, end_offset: innermost_dot, replacement: String::new() });
            }
        } else {
            let op_end = self.skip_whitespace_right(lhs_op_span.1);
            edits.push(Edit { start_offset: lhs_span.0, end_offset: op_end, replacement: String::new() });

            if let Some((recv_start, recv_end)) = self.find_receiver_global_offset(actual_rhs, rhs_src, checked_var_src, rhs_base_offset) {
                let recv_text = &self.source[recv_start..recv_end];
                if recv_text != checked_var_src {
                    edits.push(Edit { start_offset: recv_start, end_offset: recv_end, replacement: checked_var_src.to_string() });
                }
            }
        }

        for &dot_pos in &dot_positions {
            if dot_pos < self.source.len() && self.source.as_bytes()[dot_pos] == b'.'
                && (dot_pos == 0 || self.source.as_bytes()[dot_pos - 1] != b'&')
            {
                edits.push(Edit { start_offset: dot_pos, end_offset: dot_pos, replacement: "&".to_string() });
            }
        }

        edits
    }

    fn skip_whitespace_left(&self, pos: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut p = pos;
        while p > 0 && matches!(bytes[p - 1], b' ' | b'\t') { p -= 1; }
        p
    }

    fn skip_whitespace_right(&self, pos: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut p = pos;
        while p < bytes.len() && matches!(bytes[p], b' ' | b'\t') { p += 1; }
        p
    }

    fn find_receiver_global_offset(&self, rhs_node: &Node, rhs_src: &str, checked_var_src: &str, rhs_base_offset: usize) -> Option<Span> {
        let call = as_call(rhs_node)?;
        let recv = call.receiver()?;
        if src_match_ignoring_safe_nav(node_src(rhs_src, &recv), checked_var_src) {
            let loc = recv.location();
            Some((rhs_base_offset + loc.start_offset(), rhs_base_offset + loc.end_offset()))
        } else {
            self.find_receiver_global_offset(&recv, rhs_src, checked_var_src, rhs_base_offset)
        }
    }

    fn collect_chain_dots(&self, node: &Node, rhs_src: &str, checked_var_src: &str, rhs_base_offset: usize, dots: &mut Vec<usize>) {
        if let Some(call) = as_call(node) {
            if let Some(recv) = call.receiver() {
                if let Some(dot_loc) = call.call_operator_loc() {
                    let dot_global = rhs_base_offset + dot_loc.start_offset();
                    let dot_end = rhs_base_offset + dot_loc.end_offset();
                    let dot_src = &self.source[dot_global..dot_end];
                    if dot_src == "." || dot_src == "&." {
                        dots.push(dot_global);
                    }
                }
                if src_match_ignoring_safe_nav(node_src(rhs_src, &recv), checked_var_src) {
                    return;
                }
                self.collect_chain_dots(&recv, rhs_src, checked_var_src, rhs_base_offset, dots);
            }
        }
    }

    fn not_nil_receiver_src<'b>(&self, parent_src: &'b str, node: &Node) -> Option<&'b str> {
        let call = as_call(node)?;
        if !name_eq(call.name().as_slice(), "!") { return None; }
        let recv = call.receiver()?;
        let inner = as_call(&recv)?;
        if !name_eq(inner.name().as_slice(), "nil?") { return None; }
        inner.receiver().map(|r| node_src(parent_src, &r))
    }

    fn extract_and_lhs_src<'b>(&self, parent_src: &'b str, lhs: &Node) -> Option<&'b str> {
        if is_simple_var(lhs) {
            return Some(node_src(parent_src, lhs));
        }
        let call = as_call(lhs)?;
        if call.call_operator_loc().is_some() || call.receiver().is_some() {
            Some(node_src(parent_src, lhs))
        } else {
            None
        }
    }

    fn is_valid_and_lhs(&self, lhs: &Node) -> bool {
        is_simple_var(lhs) || as_call(lhs).map_or(false, |c| {
            c.call_operator_loc().is_some() || c.receiver().is_some()
        })
    }

    fn unwrap_parens_parsed<'b>(&self, node: &Node<'b>) -> Node<'b> {
        if let Node::ParenthesesNode { .. } = node {
            let paren = node.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Node::StatementsNode { .. } = &body {
                    let stmts = body.as_statements_node().unwrap();
                    let body_list = stmts.body();
                    if body_list.len() == 1 {
                        return self.unwrap_parens_parsed(&body_list.iter().next().unwrap());
                    }
                } else {
                    return self.unwrap_parens_parsed(&body);
                }
            }
        }
        dup_node(node)
    }

    fn node_contains_or(&self, node: &Node) -> bool {
        match node {
            Node::OrNode { .. } => true,
            Node::ParenthesesNode { .. } => {
                if let Some(body) = node.as_parentheses_node().unwrap().body() {
                    if let Node::StatementsNode { .. } = &body {
                        return body.as_statements_node().unwrap().body().iter()
                            .any(|stmt| self.node_contains_or(&stmt));
                    }
                    return self.node_contains_or(&body);
                }
                false
            }
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                self.node_contains_or(&and.left()) || self.node_contains_or(&and.right())
            }
            _ => false,
        }
    }

    fn add_safe_nav_all(&self, method_src: &str, var_src: &str) -> String {
        let norm_var = var_src.replace("&.", ".");
        let (pos, matched_len) = if let Some(p) = method_src.find(var_src) {
            (p, var_src.len())
        } else if let Some(p) = method_src.find(&norm_var) {
            (p, norm_var.len())
        } else {
            return method_src.to_string();
        };

        let before = &method_src[..pos];
        let after = &method_src[pos + matched_len..];

        let mut result = String::new();
        result.push_str(before);
        result.push_str(var_src);

        let mut chars = after.chars().peekable();
        let mut depth = 0i32;
        let mut in_string = false;
        let mut string_delim = '"';

        while let Some(ch) = chars.next() {
            match ch {
                '"' | '\'' if !in_string => { in_string = true; string_delim = ch; result.push(ch); }
                c if in_string && c == string_delim => { in_string = false; result.push(ch); }
                _ if in_string => { result.push(ch); }
                '(' | '[' | '{' => { depth += 1; result.push(ch); }
                ')' | ']' | '}' => { depth -= 1; result.push(ch); }
                '.' if depth == 0 => {
                    if chars.peek() == Some(&'.') {
                        result.push('.');
                    } else if result.ends_with('&') {
                        result.push('.');
                    } else {
                        result.push_str("&.");
                    }
                }
                '&' if depth == 0 && chars.peek() == Some(&'.') => { result.push('&'); }
                _ => { result.push(ch); }
            }
        }
        result
    }
}

fn dup_node<'pr>(node: &Node<'pr>) -> Node<'pr> {
    match node {
        Node::LocalVariableReadNode { .. } => node.as_local_variable_read_node().unwrap().as_node(),
        Node::ConstantReadNode { .. } => node.as_constant_read_node().unwrap().as_node(),
        Node::CallNode { .. } => node.as_call_node().unwrap().as_node(),
        Node::AndNode { .. } => node.as_and_node().unwrap().as_node(),
        Node::OrNode { .. } => node.as_or_node().unwrap().as_node(),
        Node::ParenthesesNode { .. } => node.as_parentheses_node().unwrap().as_node(),
        Node::NilNode { .. } => node.as_nil_node().unwrap().as_node(),
        Node::IfNode { .. } => node.as_if_node().unwrap().as_node(),
        Node::UnlessNode { .. } => node.as_unless_node().unwrap().as_node(),
        Node::ElseNode { .. } => node.as_else_node().unwrap().as_node(),
        Node::StatementsNode { .. } => node.as_statements_node().unwrap().as_node(),
        Node::BreakNode { .. } => node.as_break_node().unwrap().as_node(),
        Node::NextNode { .. } => node.as_next_node().unwrap().as_node(),
        Node::ReturnNode { .. } => node.as_return_node().unwrap().as_node(),
        Node::YieldNode { .. } => node.as_yield_node().unwrap().as_node(),
        Node::IntegerNode { .. } => node.as_integer_node().unwrap().as_node(),
        Node::FloatNode { .. } => node.as_float_node().unwrap().as_node(),
        Node::StringNode { .. } => node.as_string_node().unwrap().as_node(),
        Node::SymbolNode { .. } => node.as_symbol_node().unwrap().as_node(),
        Node::TrueNode { .. } => node.as_true_node().unwrap().as_node(),
        Node::FalseNode { .. } => node.as_false_node().unwrap().as_node(),
        Node::SelfNode { .. } => node.as_self_node().unwrap().as_node(),
        Node::InstanceVariableReadNode { .. } => node.as_instance_variable_read_node().unwrap().as_node(),
        Node::ClassVariableReadNode { .. } => node.as_class_variable_read_node().unwrap().as_node(),
        Node::GlobalVariableReadNode { .. } => node.as_global_variable_read_node().unwrap().as_node(),
        Node::ConstantPathNode { .. } => node.as_constant_path_node().unwrap().as_node(),
        Node::BlockNode { .. } => node.as_block_node().unwrap().as_node(),
        Node::ArrayNode { .. } => node.as_array_node().unwrap().as_node(),
        Node::HashNode { .. } => node.as_hash_node().unwrap().as_node(),
        Node::InterpolatedStringNode { .. } => node.as_interpolated_string_node().unwrap().as_node(),
        Node::RegularExpressionNode { .. } => node.as_regular_expression_node().unwrap().as_node(),
        Node::BeginNode { .. } => node.as_begin_node().unwrap().as_node(),
        Node::RangeNode { .. } => node.as_range_node().unwrap().as_node(),
        Node::LambdaNode { .. } => node.as_lambda_node().unwrap().as_node(),
        Node::DefNode { .. } => node.as_def_node().unwrap().as_node(),
        Node::ClassNode { .. } => node.as_class_node().unwrap().as_node(),
        Node::ModuleNode { .. } => node.as_module_node().unwrap().as_node(),
        Node::LocalVariableWriteNode { .. } => node.as_local_variable_write_node().unwrap().as_node(),
        Node::InstanceVariableWriteNode { .. } => node.as_instance_variable_write_node().unwrap().as_node(),
        Node::MultiWriteNode { .. } => node.as_multi_write_node().unwrap().as_node(),
        Node::SplatNode { .. } => node.as_splat_node().unwrap().as_node(),
        _ => {
            #[cfg(debug_assertions)]
            panic!("dup_node: unhandled node type");
            #[cfg(not(debug_assertions))]
            node.as_nil_node().unwrap().as_node()
        }
    }
}

impl<'a> Visit<'_> for SafeNavVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless(node);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        self.check_and(node);
    }
}
