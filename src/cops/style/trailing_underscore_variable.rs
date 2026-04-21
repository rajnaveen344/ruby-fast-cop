//! Style/TrailingUnderscoreVariable - Don't use trailing `_` in multi-assignment.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/trailing_underscore_variable.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/TrailingUnderscoreVariable";

pub struct TrailingUnderscoreVariable {
    allow_named: bool,
}

impl TrailingUnderscoreVariable {
    pub fn new(allow_named: bool) -> Self {
        Self { allow_named }
    }
}

impl Default for TrailingUnderscoreVariable {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Cop for TrailingUnderscoreVariable {
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
            allow_named: self.allow_named,
            offenses: vec![],
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    allow_named: bool,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        self.process_multi_write(node);
        // Don't call default visitor - we handle children manually
    }
}

impl<'a> Visitor<'a> {
    fn process_multi_write(&mut self, node: &ruby_prism::MultiWriteNode) {
        let targets = collect_targets_mw(node);
        let node_source = self.node_src(node.location().start_offset(), node.location().end_offset());

        // Main node offense
        if let Some((range_start, range_end)) = self.find_main_offense_range_mw(node, &targets) {
            let good_code = remove_range(&node_source, range_start - node.location().start_offset(),
                                          range_end - node.location().start_offset());
            let msg = format!("Do not use trailing `_`s in parallel assignment. Prefer `{}`.", good_code);
            let offense = self.ctx.offense_with_range(
                COP_NAME, &msg, Severity::Convention, range_start, range_end,
            );
            let correction = Correction::delete(range_start, range_end);
            self.offenses.push(offense.with_correction(correction));
        }

        // Children offenses (nested multi-target nodes)
        self.process_children_offenses(node, &targets);
    }

    fn process_children_offenses(&mut self, parent_mw: &ruby_prism::MultiWriteNode, targets: &[Node]) {
        for target in targets {
            if let Some(mt) = target.as_multi_target_node() {
                let sub_targets = collect_targets_mt(&mt);

                if let Some((range_start, range_end)) = self.find_nested_offense_range(&mt, &sub_targets) {
                    // For nested offenses, the message references the PARENT multi-write node
                    // with this specific nested range removed
                    let parent_src = self.node_src(parent_mw.location().start_offset(), parent_mw.location().end_offset());
                    let good_code = remove_range(&parent_src,
                                                  range_start - parent_mw.location().start_offset(),
                                                  range_end - parent_mw.location().start_offset());
                    let msg = format!("Do not use trailing `_`s in parallel assignment. Prefer `{}`.", good_code);
                    let offense = self.ctx.offense_with_range(
                        COP_NAME, &msg, Severity::Convention, range_start, range_end,
                    );
                    let correction = Correction::delete(range_start, range_end);
                    self.offenses.push(offense.with_correction(correction));
                }

                // Recurse into sub-targets for deeper nesting
                // For deeper nesting, we still reference the parent_mw for messages
                self.process_nested_children(parent_mw, &sub_targets);
            }
        }
    }

    fn process_nested_children(&mut self, parent_mw: &ruby_prism::MultiWriteNode, targets: &[Node]) {
        for target in targets {
            if let Some(mt) = target.as_multi_target_node() {
                let sub_targets = collect_targets_mt(&mt);

                if let Some((range_start, range_end)) = self.find_nested_offense_range(&mt, &sub_targets) {
                    let parent_src = self.node_src(parent_mw.location().start_offset(), parent_mw.location().end_offset());
                    let good_code = remove_range(&parent_src,
                                                  range_start - parent_mw.location().start_offset(),
                                                  range_end - parent_mw.location().start_offset());
                    let msg = format!("Do not use trailing `_`s in parallel assignment. Prefer `{}`.", good_code);
                    let offense = self.ctx.offense_with_range(
                        COP_NAME, &msg, Severity::Convention, range_start, range_end,
                    );
                    let correction = Correction::delete(range_start, range_end);
                    self.offenses.push(offense.with_correction(correction));
                }

                self.process_nested_children(parent_mw, &sub_targets);
            }
        }
    }

    fn find_main_offense_range_mw(&self, node: &ruby_prism::MultiWriteNode, targets: &[Node]) -> Option<(usize, usize)> {
        let first_offense_idx = self.find_first_offense_idx(targets)?;

        if first_offense_idx == 0 {
            // All variables are underscores - remove entire LHS
            let lhs_start = if node.lparen_loc().is_some() {
                node.lparen_loc().unwrap().start_offset()
            } else {
                node.location().start_offset()
            };
            let rhs_start = node.value().location().start_offset();
            return Some((lhs_start, rhs_start));
        }

        // Partial: from first offense to operator
        let offense_start = targets[first_offense_idx].location().start_offset();

        if node.lparen_loc().is_some() {
            // Parenthesized: range_for_parentheses
            // (mirroring RuboCop: offense.begin_pos - 1, left.end_pos - 1)
            let off_start = offense_start - 1;
            let rparen = node.rparen_loc().unwrap();
            let off_end = rparen.end_offset() - 1; // before ')'
            Some((off_start, off_end))
        } else {
            // range from first offense to operator
            let operator_start = node.operator_loc().start_offset();
            Some((offense_start, operator_start))
        }
    }

    fn find_nested_offense_range(&self, mt: &ruby_prism::MultiTargetNode, targets: &[Node]) -> Option<(usize, usize)> {
        let first_offense_idx = self.find_first_offense_idx(targets)?;

        let offense_start = targets[first_offense_idx].location().start_offset();

        if first_offense_idx == 0 && mt.lparen_loc().is_some() {
            // All variables are underscores in nested parenthesized target
            // Range from before first offense comma to before rparen
            let off_start = scan_back_past_comma_space(self.ctx.source, offense_start);
            let off_end = mt.rparen_loc().unwrap().end_offset() - 1;
            Some((off_start, off_end))
        } else if mt.lparen_loc().is_some() {
            // Partial parenthesized: from one char before first offense to before rparen
            // (mirroring RuboCop's range_for_parentheses: offense.begin_pos - 1, left.end_pos - 1)
            let off_start = offense_start - 1;
            let off_end = mt.rparen_loc().unwrap().end_offset() - 1;
            Some((off_start, off_end))
        } else {
            // Non-parenthesized nested target (unusual)
            let off_start = scan_back_past_comma_space(self.ctx.source, offense_start);
            let off_end = mt.location().end_offset();
            Some((off_start, off_end))
        }
    }

    fn find_first_offense_idx(&self, targets: &[Node]) -> Option<usize> {
        // Walk from end, find earliest consecutive trailing underscore
        let mut first_offense_idx: Option<usize> = None;

        for i in (0..targets.len()).rev() {
            let target = &targets[i];
            if !self.is_underscore_target(target) {
                break;
            }
            first_offense_idx = Some(i);
        }

        let idx = first_offense_idx?;

        // Check if there's a non-underscore splat before the first offense
        if self.has_splat_before(targets, idx) {
            return None;
        }

        Some(idx)
    }

    fn is_underscore_target(&self, node: &Node) -> bool {
        match node {
            Node::LocalVariableTargetNode { .. } => {
                let name = String::from_utf8_lossy(
                    node.as_local_variable_target_node().unwrap().name().as_slice(),
                );
                if self.allow_named && name != "_" && name.starts_with('_') {
                    return false;
                }
                name.starts_with('_')
            }
            Node::SplatNode { .. } => {
                let splat = node.as_splat_node().unwrap();
                if let Some(expr) = splat.expression() {
                    if let Some(lvtn) = expr.as_local_variable_target_node() {
                        let name = String::from_utf8_lossy(lvtn.name().as_slice());
                        if self.allow_named && name != "_" && name.starts_with('_') {
                            return false;
                        }
                        name.starts_with('_')
                    } else {
                        false
                    }
                } else {
                    // bare splat * without name - not a valid target here
                    false
                }
            }
            _ => false,
        }
    }

    fn has_splat_before(&self, targets: &[Node], first_offense_idx: usize) -> bool {
        // Check if there's a non-underscore splat variable before the first offense
        for target in &targets[..first_offense_idx] {
            if let Node::SplatNode { .. } = target {
                let splat = target.as_splat_node().unwrap();
                // If the splat itself is an underscore target, it doesn't block
                if let Some(expr) = splat.expression() {
                    if let Some(lvtn) = expr.as_local_variable_target_node() {
                        let name = String::from_utf8_lossy(lvtn.name().as_slice());
                        if name.starts_with('_') {
                            // Underscore splat - this counts as an underscore itself,
                            // doesn't block. But if we allow named and it's named, it blocks.
                            if self.allow_named && name != "_" {
                                return true; // named underscore splat blocks
                            }
                            continue; // underscore splat doesn't block
                        }
                    }
                }
                return true; // non-underscore splat blocks
            }
        }
        false
    }

    fn node_src(&self, start: usize, end: usize) -> String {
        self.ctx.source[start..end].to_string()
    }
}

fn collect_targets_mw<'pr>(node: &ruby_prism::MultiWriteNode<'pr>) -> Vec<Node<'pr>> {
    node.lefts().iter()
        .chain(node.rest().into_iter())
        .chain(node.rights().iter())
        .filter(|n| !matches!(n, Node::ImplicitRestNode { .. }))
        .collect()
}

fn collect_targets_mt<'pr>(node: &ruby_prism::MultiTargetNode<'pr>) -> Vec<Node<'pr>> {
    node.lefts().iter()
        .chain(node.rest().into_iter())
        .chain(node.rights().iter())
        .filter(|n| !matches!(n, Node::ImplicitRestNode { .. }))
        .collect()
}

/// Remove a range from a string
fn remove_range(s: &str, start: usize, end: usize) -> String {
    let mut result = String::new();
    result.push_str(&s[..start]);
    result.push_str(&s[end..]);
    result
}

/// Scan backwards past comma and whitespace
fn scan_back_past_comma_space(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = offset;
    // Skip backwards over whitespace
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    // Skip backwards over comma
    if i > 0 && bytes[i - 1] == b',' {
        i -= 1;
    }
    // Skip backwards over whitespace again
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    i
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { allow_named_underscore_variables: bool }
impl Default for Cfg {
    fn default() -> Self { Self { allow_named_underscore_variables: true } }
}

crate::register_cop!("Style/TrailingUnderscoreVariable", |cfg| {
    let c: Cfg = cfg.typed("Style/TrailingUnderscoreVariable");
    Some(Box::new(TrailingUnderscoreVariable::new(c.allow_named_underscore_variables)))
});
