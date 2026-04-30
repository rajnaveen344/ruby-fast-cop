//! Style/For - Enforce consistency between `for` loops and `each`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/for.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/For";
const EACH_MSG: &str = "Prefer `each` over `for`.";
const FOR_MSG: &str = "Prefer `for` over `each`.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnforcedStyle {
    Each,
    For,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::Each
    }
}

#[derive(Default)]
pub struct For {
    style: EnforcedStyle,
}

impl For {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Cop for For {
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
        let mut v = Visitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

fn collection_needs_parens(node: &Node) -> bool {
    if let Some(call) = node.as_call_node() {
        let name = String::from_utf8_lossy(call.name().as_slice());
        if !name.is_empty() {
            let first = name.chars().next().unwrap();
            if !first.is_alphanumeric() && first != '_' {
                return true;
            }
        }
        return false;
    }
    if node.as_range_node().is_some() { return true }
    if node.as_and_node().is_some() || node.as_or_node().is_some() { return true }
    false
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        if self.style == EnforcedStyle::Each {
            // Report: range = `for IDX in COLL [do]`
            let start = node.location().start_offset();
            let end = match node.do_keyword_loc() {
                Some(do_loc) => do_loc.end_offset(),
                None => node.collection().location().end_offset(),
            };
            let src = self.ctx.source;
            let idx_loc = node.index().location();
            let coll_loc = node.collection().location();
            let idx_src = &src[idx_loc.start_offset()..idx_loc.end_offset()];
            let coll_src = &src[coll_loc.start_offset()..coll_loc.end_offset()];
            let coll_node = node.collection();
            let needs_parens = collection_needs_parens(&coll_node);
            let dot = if let Some(call) = coll_node.as_call_node() {
                if call.is_safe_navigation() { "&." } else { "." }
            } else { "." };
            let coll_text = if needs_parens && !coll_src.starts_with('(') {
                format!("({})", coll_src)
            } else {
                coll_src.to_string()
            };
            let replacement = format!("{}{}each do |{}|", coll_text, dot, idx_src);
            self.offenses.push(self.ctx.offense_with_range(
                COP_NAME,
                EACH_MSG,
                Severity::Convention,
                start,
                end,
            ).with_correction(Correction::replace(start, end, &replacement)));
        }
        ruby_prism::visit_for_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.style == EnforcedStyle::For {
            let name = node_name!(node);
            if name == "each" {
                // Only block form w/ do...end (multiline) and must have receiver
                if node.receiver().is_some() {
                    if let Some(block) = node.block() {
                        if let Some(bn) = block.as_block_node() {
                            let node_start = node.location().start_offset();
                            let node_end = node.location().end_offset();
                            let src = &self.ctx.source[node_start..node_end];
                            // Multiline only (for is useless for single-line each { })
                            if src.contains('\n') {
                                // Skip brace-based block
                                let opening = bn.opening_loc();
                                let opening_src =
                                    &self.ctx.source[opening.start_offset()..opening.end_offset()];
                                if opening_src == "do" {
                                    // Range: `recv.each do [|params|]`
                                    let end = if let Some(params) = bn.parameters() {
                                        if params.as_block_parameters_node().is_some() {
                                            params.location().end_offset()
                                        } else {
                                            opening.end_offset()
                                        }
                                    } else {
                                        opening.end_offset()
                                    };
                                    // Build replacement: `for IDX in COLL do`
                                    let recv = node.receiver().unwrap();
                                    let recv_loc = recv.location();
                                    let recv_src = &self.ctx.source[recv_loc.start_offset()..recv_loc.end_offset()];
                                    // Strip surrounding parens around collection: ParenthesesNode
                                    let coll_text = if recv.as_parentheses_node().is_some()
                                        && recv_src.starts_with('(') && recv_src.ends_with(')')
                                    {
                                        recv_src[1..recv_src.len()-1].trim().to_string()
                                    } else {
                                        recv_src.to_string()
                                    };
                                    let idx_text = if let Some(params) = bn.parameters() {
                                        if let Some(bp) = params.as_block_parameters_node() {
                                            // Get params source
                                            let pl = bp.location();
                                            let psrc = &self.ctx.source[pl.start_offset()..pl.end_offset()];
                                            // strip surrounding `|...|`
                                            psrc.trim_matches('|').to_string()
                                        } else {
                                            "_".to_string()
                                        }
                                    } else {
                                        "_".to_string()
                                    };
                                    let replacement = format!("for {} in {} do", idx_text, coll_text);
                                    self.offenses.push(self.ctx.offense_with_range(
                                        COP_NAME,
                                        FOR_MSG,
                                        Severity::Convention,
                                        node_start,
                                        end,
                                    ).with_correction(Correction::replace(node_start, end, &replacement)));
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/For", |cfg| {
    let c: Cfg = cfg.typed("Style/For");
    let style = match c.enforced_style.as_str() {
        "for" => EnforcedStyle::For,
        _ => EnforcedStyle::Each,
    };
    Some(Box::new(For::with_style(style)))
});
