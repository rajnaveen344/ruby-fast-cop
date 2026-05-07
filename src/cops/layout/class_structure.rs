//! Layout/ClassStructure - Enforces class element ordering.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/class_structure.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

const COP_NAME: &str = "Layout/ClassStructure";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visibility {
    Public,
    Protected,
    Private,
}

pub struct ClassStructure {
    expected_order: Vec<String>,
    /// Maps method/macro name → category key (e.g. "attr_accessor" → "attribute_macros")
    name_to_category: HashMap<String, String>,
}

impl Default for ClassStructure {
    fn default() -> Self {
        Self {
            expected_order: Vec::new(),
            name_to_category: HashMap::new(),
        }
    }
}

impl ClassStructure {
    pub fn new(expected_order: Vec<String>, categories: HashMap<String, Vec<String>>) -> Self {
        let mut name_to_category = HashMap::new();
        for (cat, names) in categories {
            for n in names {
                name_to_category.insert(n, cat.clone());
            }
        }
        Self {
            expected_order,
            name_to_category,
        }
    }

    fn find_category(&self, name: &str) -> Option<&str> {
        self.name_to_category.get(name).map(|s| s.as_str())
    }

    /// Returns Some(category) for nodes that should be classified, None for skip-only nodes
    /// (visibility modifiers, private_constant, dynamic constants, etc.).
    fn classify(
        &self,
        node: &Node,
        visibility: Visibility,
        private_constants: &std::collections::HashSet<String>,
        private_named: &std::collections::HashSet<String>,
        protected_named: &std::collections::HashSet<String>,
    ) -> Option<String> {
        match node {
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                let raw_name = def.name();
                let name = String::from_utf8_lossy(raw_name.as_slice()).into_owned();
                // self.x → public_class_methods
                if def.receiver().is_some() {
                    return Some("public_class_methods".to_string());
                }
                if name == "initialize" {
                    return Some("initializer".to_string());
                }
                let v = if private_named.contains(&name) {
                    Visibility::Private
                } else if protected_named.contains(&name) {
                    Visibility::Protected
                } else {
                    visibility
                };
                Some(format!("{}_methods", visibility_str(v)))
            }
            Node::ConstantWriteNode { .. } => {
                let cw = node.as_constant_write_node().unwrap();
                let raw = cw.name();
                let const_name = String::from_utf8_lossy(raw.as_slice()).into_owned();
                if private_constants.contains(&const_name) {
                    return None;
                }
                // Check categories override for "constants" key
                if let Some(cat) = self.find_category("constants") {
                    return Some(cat.to_string());
                }
                Some("constants".to_string())
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if call.receiver().is_some() {
                    return None;
                }
                let raw_name = node_name!(call);
                let name: String = raw_name.as_ref().to_string();

                // visibility modifier (no args) — skip; caller handles toggling.
                if matches!(name.as_str(), "private" | "public" | "protected") {
                    let arg_count = call
                        .arguments()
                        .map(|a| a.arguments().iter().count())
                        .unwrap_or(0);
                    if arg_count == 0 {
                        return None;
                    }
                    // private :foo / private def foo
                    // def_modifier? → call has a single DefNode arg
                    if let Some(args) = call.arguments() {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if arg_list.len() == 1 {
                            if matches!(arg_list[0], Node::DefNode { .. }) {
                                return Some(format!("{}_methods", name));
                            }
                        }
                    }
                    // private :foo, :bar → not classified (acts as visibility marker for prior defs)
                    return None;
                }

                // private_constant marker
                if name == "private_constant" {
                    return None;
                }

                let category = self.find_category(&name);
                let key = category.unwrap_or(&name).to_string();
                let visibility_key = format!("{}_{}", visibility_str(visibility), key);
                if self.expected_order.iter().any(|e| e == &visibility_key) {
                    Some(visibility_key)
                } else {
                    Some(key)
                }
            }
            _ => None,
        }
    }

    /// `begin_pos_with_comment`: start of the first preceding whole-line comment (or node's line start).
    /// Returns the byte offset of the first character of the node's line, walking backwards
    /// through any preceding comment-only lines.
    fn begin_pos_with_comment(source: &str, node_start: usize) -> usize {
        let bytes = source.as_bytes();
        // Find start of node's line
        let mut line_start = node_start;
        while line_start > 0 && bytes[line_start - 1] != b'\n' {
            line_start -= 1;
        }
        // Walk upward through preceding comment lines
        let mut result = line_start;
        loop {
            if result == 0 { break; }
            // Previous line end (the \n before result)
            let prev_line_end = result - 1; // points to the \n
            let mut prev_line_start = prev_line_end;
            while prev_line_start > 0 && bytes[prev_line_start - 1] != b'\n' {
                prev_line_start -= 1;
            }
            let prev_line = &source[prev_line_start..prev_line_end];
            if prev_line.trim().starts_with('#') {
                result = prev_line_start;
            } else {
                break;
            }
        }
        result
    }

    /// `end_position_for`: end of node's last line (past newline), handles heredoc
    fn end_position_for(source: &str, node: &Node) -> usize {
        // For heredoc constant assignments, find the real end (past closing EOS marker)
        if let Some(pos) = Self::find_heredoc_end(source, node) {
            return pos;
        }

        let node_end = node.location().end_offset();
        let bytes = source.as_bytes();
        if node_end == 0 { return 0; }
        // If node_end already points past a '\n', return it as-is
        if node_end > 0 && node_end <= bytes.len() && bytes[node_end.saturating_sub(1)] == b'\n' {
            return node_end;
        }
        // Find end of current line
        let mut end = node_end;
        while end < bytes.len() && bytes[end] != b'\n' { end += 1; }
        if end < bytes.len() && bytes[end] == b'\n' { end += 1; }
        end
    }

    /// Finds the end position of a heredoc if this node is a constant assignment with heredoc value.
    /// Returns Some(end_offset_past_closing_marker) or None.
    fn find_heredoc_end(source: &str, node: &Node) -> Option<usize> {
        // Only handle ConstantWriteNode with a heredoc string value
        let cw = node.as_constant_write_node()?;
        let value = cw.value();

        // Find the heredoc marker in value's source
        let val_src = &source[value.location().start_offset()..value.location().end_offset()];
        let is_heredoc = val_src.starts_with("<<");
        if !is_heredoc { return None; }

        // Extract the delimiter from `<<~DELIM`, `<<-DELIM`, `<<DELIM`, `<<~"DELIM"`, etc.
        let re = regex::Regex::new(r#"<<([~-])?(['"`]?)(\w+)(['"`]?)"#).ok()?;
        let caps = re.captures(val_src)?;
        let delimiter = caps.get(3)?.as_str();

        // Body starts on the next line after the opening line
        let val_end = value.location().end_offset();
        let body_start = match source[val_end..].find('\n') {
            Some(pos) => val_end + pos + 1,
            None => return None,
        };
        if body_start >= source.len() { return None; }

        // Find closing delimiter line
        let closing_re = regex::Regex::new(
            &format!(r"(?m)^[ \t]*{}[ \t]*$", regex::escape(delimiter))
        ).ok()?;
        let m = closing_re.find(&source[body_start..])?;
        // closing line ends at body_start + m.end()
        let closing_end = body_start + m.end();
        // Include the trailing newline
        let bytes = source.as_bytes();
        let result = if closing_end < bytes.len() && bytes[closing_end] == b'\n' {
            closing_end + 1
        } else {
            closing_end
        };
        Some(result)
    }

    /// `source_range_with_comment`: [begin_pos_with_comment .. end_position_for]
    fn source_range_with_comment(source: &str, node: &Node) -> (usize, usize) {
        let start = Self::begin_pos_with_comment(source, node.location().start_offset());
        let end = Self::end_position_for(source, node);
        (start, end)
    }

    /// `dynamic_constant?`: a constant assignment whose value is a send (non-freeze/non-literal)
    fn is_dynamic_constant(node: &Node) -> bool {
        let Some(cw) = node.as_constant_write_node() else { return false; };
        let value = cw.value();
        // Dynamic if value is a send that's not `something.freeze` where receiver is basic literal
        match &value {
            Node::CallNode { .. } => {
                let call = value.as_call_node().unwrap();
                let method = call.name().as_slice();
                if method == b"freeze" {
                    // Check if receiver is a basic literal recursively
                    if let Some(recv) = call.receiver() {
                        return !Self::is_recursive_basic_literal(&recv);
                    }
                }
                true // any other send = dynamic
            }
            _ => false,
        }
    }

    fn is_recursive_basic_literal(node: &Node) -> bool {
        match node {
            Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::StringNode { .. }
            | Node::SymbolNode { .. } | Node::NilNode { .. } | Node::TrueNode { .. }
            | Node::FalseNode { .. } => true,
            Node::ArrayNode { .. } => {
                let arr = node.as_array_node().unwrap();
                arr.elements().iter().all(|e| Self::is_recursive_basic_literal(&e))
            }
            Node::HashNode { .. } => {
                let hash = node.as_hash_node().unwrap();
                hash.elements().iter().all(|e| {
                    if let Some(assoc) = e.as_assoc_node() {
                        let key_ok = Self::is_recursive_basic_literal(&assoc.key());
                        let val_ok = Self::is_recursive_basic_literal(&assoc.value());
                        key_ok && val_ok
                    } else {
                        false
                    }
                })
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if call.name().as_slice() == b"freeze" {
                    if let Some(recv) = call.receiver() {
                        return Self::is_recursive_basic_literal(&recv);
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// `ignore_for_autocorrect?`: should we skip this sibling when looking for "previous"?
    fn ignore_for_autocorrect<'a>(
        &self,
        current: &Node,
        sibling: &Node,
        visibility: Visibility,
        private_constants: &std::collections::HashSet<String>,
        private_named: &std::collections::HashSet<String>,
        protected_named: &std::collections::HashSet<String>,
    ) -> bool {
        // If sibling is not classified / not in expected order → ignore
        let sibling_cat = match self.classify(sibling, visibility, private_constants, private_named, protected_named) {
            Some(c) => c,
            None => return true,
        };
        if self.expected_order.iter().position(|e| e == &sibling_cat).is_none() {
            return true;
        }
        // If same category as current → ignore
        let current_cat = match self.classify(current, visibility, private_constants, private_named, protected_named) {
            Some(c) => c,
            None => return true,
        };
        if sibling_cat == current_cat {
            return true;
        }
        // If current is a dynamic constant → ignore (don't autocorrect)
        if Self::is_dynamic_constant(current) {
            return true;
        }
        false
    }

    fn check_body_children<'a>(
        &self,
        children: &[Node<'a>],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        // First pass: collect private_constants list and private/protected method names
        let mut private_constants: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut private_named: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut protected_named: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for child in children {
            if let Node::CallNode { .. } = child {
                let call = child.as_call_node().unwrap();
                if call.receiver().is_some() {
                    continue;
                }
                let n = node_name!(call);
                let n_str = n.as_ref();
                let arg_count = call
                    .arguments()
                    .map(|a| a.arguments().iter().count())
                    .unwrap_or(0);
                let target_set: Option<&mut std::collections::HashSet<String>> = match n_str {
                    "private_constant" => Some(&mut private_constants),
                    "private" if arg_count > 0 => Some(&mut private_named),
                    "protected" if arg_count > 0 => Some(&mut protected_named),
                    _ => None,
                };
                let Some(target) = target_set else {
                    continue;
                };
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    // Skip `private def foo` form — DefNode arg means modifier-style, not name list
                    if arg_list.iter().any(|a| matches!(a, Node::DefNode { .. })) {
                        continue;
                    }
                    for a in &arg_list {
                        match a {
                            Node::SymbolNode { .. } => {
                                let sym = a.as_symbol_node().unwrap();
                                let bytes = sym.unescaped();
                                let b: &[u8] = bytes.as_ref();
                                let s = String::from_utf8_lossy(b).into_owned();
                                target.insert(s);
                            }
                            Node::StringNode { .. } => {
                                let st = a.as_string_node().unwrap();
                                let bytes = st.unescaped();
                                let b: &[u8] = bytes.as_ref();
                                let s = String::from_utf8_lossy(b).into_owned();
                                target.insert(s);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Second pass: walk children, track visibility, classify, emit offense if out-of-order.
        // Also track which children are "classified" (in order) for correction purposes.
        let mut visibility = Visibility::Public;
        let mut prev_index: i32 = -1;
        // classified_children: (child_idx_in_children, category, order_idx)
        let mut classified_children: Vec<(usize, String, i32)> = Vec::new();

        for (ci, child) in children.iter().enumerate() {
            // Visibility-only marker?
            if let Node::CallNode { .. } = child {
                let call = child.as_call_node().unwrap();
                if call.receiver().is_none() {
                    let n = node_name!(call);
                    let arg_count = call
                        .arguments()
                        .map(|a| a.arguments().iter().count())
                        .unwrap_or(0);
                    if arg_count == 0
                        && matches!(n.as_ref(), "private" | "protected" | "public")
                    {
                        visibility = match n.as_ref() {
                            "private" => Visibility::Private,
                            "protected" => Visibility::Protected,
                            _ => Visibility::Public,
                        };
                        continue;
                    }
                }
            }

            let Some(category) = self.classify(
                child,
                visibility,
                &private_constants,
                &private_named,
                &protected_named,
            ) else {
                continue;
            };
            let Some(idx) = self.expected_order.iter().position(|e| e == &category) else {
                continue;
            };
            let idx = idx as i32;

            if idx < prev_index {
                let prev = &self.expected_order[prev_index as usize];
                let message = format!(
                    "`{}` is supposed to appear before `{}`.",
                    category, prev
                );
                let loc = child.location();

                // Build correction: find previous classified sibling that we shouldn't ignore
                let correction = if !Self::is_dynamic_constant(child) {
                    // Find "previous" = first classified child (in left-sibling order) that's not ignored
                    // Use "first forward" (not "last reverse") so that single-pass correction
                    // reproduces RuboCop's multi-pass final stable state.
                    let previous = classified_children.iter().find(|(pi, _, _)| {
                        let prev_node = &children[*pi];
                        !self.ignore_for_autocorrect(
                            child, prev_node, visibility,
                            &private_constants, &private_named, &protected_named
                        )
                    }).map(|(pi, _, _)| &children[*pi]);

                    if let Some(prev_node) = previous {
                        let (prev_start, _prev_end) = Self::source_range_with_comment(ctx.source, prev_node);
                        let current_start = Self::begin_pos_with_comment(ctx.source, child.location().start_offset());
                        let mut group_end = Self::end_position_for(ctx.source, child);

                        // Scan forward from ci+1 for contiguous same-category non-dynamic siblings.
                        // We only extend the group while the next classified node is same-category.
                        // Stop at a different-category classified node (but skip unclassified/visibility markers).
                        let mut j = ci + 1;
                        let mut local_vis = visibility;
                        while j < children.len() {
                            let next = &children[j];
                            // Handle visibility marker (unclassified)
                            if let Some(call) = next.as_call_node() {
                                if call.receiver().is_none() {
                                    let nm = node_name!(call);
                                    let ac = call.arguments().map(|a| a.arguments().iter().count()).unwrap_or(0);
                                    if ac == 0 && matches!(nm.as_ref(), "private"|"protected"|"public") {
                                        local_vis = match nm.as_ref() {
                                            "private" => Visibility::Private,
                                            "protected" => Visibility::Protected,
                                            _ => Visibility::Public,
                                        };
                                        j += 1;
                                        continue;
                                    }
                                }
                            }

                            // Classify this node
                            let next_cat = self.classify(next, local_vis, &private_constants, &private_named, &protected_named);
                            match next_cat {
                                Some(ref nc) if nc == &category && !Self::is_dynamic_constant(next) => {
                                    // Same category, non-dynamic → extend group to include this sibling
                                    group_end = Self::end_position_for(ctx.source, next);
                                    j += 1;
                                }
                                None => {
                                    // Unclassified (e.g. private_constant) → skip
                                    j += 1;
                                }
                                _ => {
                                    // Different classified category → stop grouping
                                    break;
                                }
                            }
                        }

                        // Special case: if child is a heredoc constant and there's a blank line
                        // immediately before current_start, include it in the deletion and move it
                        // to after the heredoc in the destination (so RuboCop's blank-line placement
                        // is reproduced correctly in a single pass).
                        let is_heredoc = Self::find_heredoc_end(ctx.source, child).is_some();
                        let has_preceding_blank = is_heredoc
                            && current_start >= 2
                            && ctx.source.as_bytes()[current_start - 1] == b'\n'
                            && ctx.source.as_bytes()[current_start - 2] == b'\n';

                        let (actual_delete_start, group_text) = if has_preceding_blank {
                            // Include the blank line (\n at current_start - 1) in the delete range.
                            // Move the blank to end of group_text so it follows the heredoc at destination.
                            let raw = &ctx.source[current_start..group_end];
                            let text = raw.to_string() + "\n";
                            (current_start - 1, text)
                        } else {
                            (current_start, ctx.source[current_start..group_end].to_string())
                        };

                        let edits = vec![
                            // Insert group text before prev_node's line start
                            Edit {
                                start_offset: prev_start,
                                end_offset: prev_start,
                                replacement: group_text,
                            },
                            // Delete the original range (contiguous, adjusted for blank line)
                            Edit {
                                start_offset: actual_delete_start,
                                end_offset: group_end,
                                replacement: String::new(),
                            },
                        ];
                        Some(Correction { edits })
                    } else {
                        None
                    }
                } else {
                    None
                };

                let offense = ctx.offense_with_range(
                    COP_NAME,
                    &message,
                    Severity::Convention,
                    loc.start_offset(),
                    loc.end_offset(),
                );
                if let Some(corr) = correction {
                    offenses.push(offense.with_correction(corr));
                } else {
                    offenses.push(offense);
                }
            }

            classified_children.push((ci, category, idx));
            prev_index = idx;
        }
    }

    fn check_body_node(&self, body_node: &Node, ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        // body may be a StatementsNode or a single statement.
        if let Some(stmts) = body_node.as_statements_node() {
            let children: Vec<Node> = stmts.body().iter().collect();
            self.check_body_children(&children, ctx, offenses);
        }
        // Single-statement bodies have nothing to reorder, so skip them.
    }
}

fn visibility_str(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    }
}

struct ClassStructureVisitor<'a, 'src> {
    cop: &'a ClassStructure,
    ctx: &'a CheckContext<'src>,
    offenses: Vec<Offense>,
}

impl<'a, 'src> Visit<'src> for ClassStructureVisitor<'a, 'src> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'src>) {
        if let Some(body) = node.body() {
            self.cop.check_body_node(&body, self.ctx, &mut self.offenses);
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'src>) {
        if let Some(body) = node.body() {
            self.cop.check_body_node(&body, self.ctx, &mut self.offenses);
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

impl Cop for ClassStructure {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = ClassStructureVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Layout/ClassStructure", |cfg| {
    let mut expected_order = Vec::new();
    let mut categories: HashMap<String, Vec<String>> = HashMap::new();

    if let Some(cc) = cfg.get_cop_config("Layout/ClassStructure") {
        if let Some(serde_yaml::Value::Sequence(seq)) = cc.raw.get("ExpectedOrder") {
            for v in seq {
                if let Some(s) = v.as_str() {
                    expected_order.push(s.to_string());
                }
            }
        }
        if let Some(serde_yaml::Value::Mapping(m)) = cc.raw.get("Categories") {
            for (k, v) in m {
                let Some(key) = k.as_str() else { continue };
                let Some(serde_yaml::Value::Sequence(seq)) = Some(v) else {
                    continue;
                };
                let names: Vec<String> = seq
                    .iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect();
                categories.insert(key.to_string(), names);
            }
        }
    }
    Some(Box::new(ClassStructure::new(expected_order, categories)))
});
