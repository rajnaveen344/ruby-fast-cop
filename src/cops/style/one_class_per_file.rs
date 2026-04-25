//! Style/OneClassPerFile cop
//!
//! Checks that each source file defines at most one top-level class or module.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Style/OneClassPerFile";
const MSG: &str = "Do not define multiple classes/modules at the top level in a single file.";

#[derive(Default)]
pub struct OneClassPerFile {
    allowed_classes: Vec<String>,
}

impl OneClassPerFile {
    pub fn new(allowed_classes: Vec<String>) -> Self {
        Self { allowed_classes }
    }
}

impl Cop for OneClassPerFile {
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
        let mut offenses = Vec::new();
        let mut count = 0;

        for stmt in node.statements().body().iter() {
            // Get short name + name-end offset for class or module
            let (name, name_end, start) = match &stmt {
                Node::ClassNode { .. } => {
                    let cn = stmt.as_class_node().unwrap();
                    let short = node_name!(cn);
                    let path = cn.constant_path();
                    let n_end = path.location().end_offset();
                    let s = cn.location().start_offset();
                    (short.into_owned(), n_end, s)
                }
                Node::ModuleNode { .. } => {
                    let mn = stmt.as_module_node().unwrap();
                    let short = node_name!(mn);
                    let path = mn.constant_path();
                    let n_end = path.location().end_offset();
                    let s = mn.location().start_offset();
                    (short.into_owned(), n_end, s)
                }
                _ => continue,
            };

            if self.allowed_classes.iter().any(|c| c == &name) {
                continue;
            }

            count += 1;
            if count > 1 {
                offenses.push(ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Convention,
                    start,
                    name_end,
                ));
            }
        }

        offenses
    }
}

crate::register_cop!("Style/OneClassPerFile", |cfg| {
    let allowed = cfg
        .get_cop_config("Style/OneClassPerFile")
        .and_then(|c| c.raw.get("AllowedClasses"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(OneClassPerFile::new(allowed)))
});
