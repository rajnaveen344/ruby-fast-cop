//! Lint/UnifiedInteger - Use Integer instead of Fixnum or Bignum.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Use `Integer` instead of `%s`.";

#[derive(Default)]
pub struct UnifiedInteger;

impl UnifiedInteger {
    pub fn new() -> Self { Self }
}

impl Cop for UnifiedInteger {
    fn name(&self) -> &'static str { "Lint/UnifiedInteger" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode) {
        let name = String::from_utf8_lossy(node.name().as_slice());
        if name == "Fixnum" || name == "Bignum" {
            let msg = MSG.replace("%s", name.as_ref());
            let loc = node.location();
            let mut offense = self.ctx.offense_with_range(
                "Lint/UnifiedInteger",
                &msg,
                Severity::Warning,
                loc.start_offset(),
                loc.end_offset(),
            );
            // Correction only for ruby >= 2.4
            if self.ctx.target_ruby_version >= 2.4 {
                offense = offense.with_correction(Correction::replace(
                    loc.start_offset(),
                    loc.end_offset(),
                    "Integer".to_string(),
                ));
            }
            self.offenses.push(offense);
        }
        ruby_prism::visit_constant_read_node(self, node);
    }

    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode) {
        // Only flag top-level ::Fixnum / ::Bignum (no parent, cbase-style)
        // NOT MyNamespace::Fixnum (parent is Some)
        if node.parent().is_none() {
            if let Some(const_id) = node.name() {
                let name_bytes = const_id.as_slice();
                let name = String::from_utf8_lossy(name_bytes);
                if name == "Fixnum" || name == "Bignum" {
                    let msg = MSG.replace("%s", name.as_ref());
                    let node_loc = node.location();
                    let name_loc = node.name_loc();
                    let mut offense = self.ctx.offense_with_range(
                        "Lint/UnifiedInteger",
                        &msg,
                        Severity::Warning,
                        node_loc.start_offset(),
                        node_loc.end_offset(),
                    );
                    if self.ctx.target_ruby_version >= 2.4 {
                        offense = offense.with_correction(Correction::replace(
                            name_loc.start_offset(),
                            name_loc.end_offset(),
                            "Integer".to_string(),
                        ));
                    }
                    self.offenses.push(offense);
                    // Don't recurse — would double-count via visit_constant_read_node
                    return;
                }
            }
        }
        ruby_prism::visit_constant_path_node(self, node);
    }
}

crate::register_cop!("Lint/UnifiedInteger", |_cfg| Some(Box::new(UnifiedInteger::new())));
