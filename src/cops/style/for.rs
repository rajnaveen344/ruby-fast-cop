//! Style/For - Enforce consistency between `for` loops and `each`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/for.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
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
            self.offenses.push(self.ctx.offense_with_range(
                COP_NAME,
                EACH_MSG,
                Severity::Convention,
                start,
                end,
            ));
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
                                        params.location().end_offset()
                                    } else {
                                        opening.end_offset()
                                    };
                                    self.offenses.push(self.ctx.offense_with_range(
                                        COP_NAME,
                                        FOR_MSG,
                                        Severity::Convention,
                                        node_start,
                                        end,
                                    ));
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
