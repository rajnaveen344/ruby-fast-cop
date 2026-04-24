//! Lint/NumberedParameterAssignment
//!
//! Flags `_N = ...` assignments since `_1`..`_9` are reserved for numbered
//! parameters in Ruby 3.0+.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::LocalVariableWriteNode;

pub struct NumberedParameterAssignment;

impl NumberedParameterAssignment {
    pub fn new() -> Self { Self }
}

impl Default for NumberedParameterAssignment {
    fn default() -> Self { Self }
}

impl Cop for NumberedParameterAssignment {
    fn name(&self) -> &'static str { "Lint/NumberedParameterAssignment" }

    fn severity(&self) -> Severity { Severity::Warning }

    fn check_local_variable_write(
        &self,
        node: &LocalVariableWriteNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
        let Some(num_str) = name.strip_prefix('_') else { return vec![] };
        if num_str.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
            return vec![];
        }
        let Ok(n) = num_str.parse::<u32>() else { return vec![] };

        let msg = if (1..=9).contains(&n) {
            format!("`_{}` is reserved for numbered parameter; consider another name.", n)
        } else {
            format!("`_{}` is similar to numbered parameter; consider another name.", n)
        };

        let loc = node.location();
        vec![ctx.offense_with_range(
            self.name(),
            &msg,
            self.severity(),
            loc.start_offset(),
            loc.end_offset(),
        )]
    }
}

crate::register_cop!("Lint/NumberedParameterAssignment", |_cfg| {
    Some(Box::new(NumberedParameterAssignment::new()))
});
