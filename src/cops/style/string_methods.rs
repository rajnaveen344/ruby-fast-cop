//! Style/StringMethods - Enforces consistent method names from the String class.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/string_methods.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use std::collections::HashMap;

/// Enforces the use of consistent method names from the String class.
///
/// # Examples
///
/// ```ruby
/// # bad
/// 'name'.intern
///
/// # good
/// 'name'.to_sym
/// ```
pub struct StringMethods {
    preferred_methods: HashMap<&'static str, &'static str>,
}

impl StringMethods {
    pub fn new() -> Self {
        let mut preferred_methods = HashMap::new();
        // Default RuboCop preferences
        preferred_methods.insert("intern", "to_sym");
        Self { preferred_methods }
    }
}

impl Default for StringMethods {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for StringMethods {
    fn name(&self) -> &'static str {
        "Style/StringMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = node_name!(node);

        if let Some(&preferred) = self.preferred_methods.get(method_name.as_ref()) {
            // Use message_loc() to get the location of just the method name, not the whole call
            if let Some(message_loc) = node.message_loc() {
                let offense = ctx.offense(
                    self.name(),
                    &format!("Prefer `{}` over `{}`.", preferred, method_name),
                    self.severity(),
                    &message_loc,
                );
                let correction = Correction::replace(
                    message_loc.start_offset(),
                    message_loc.end_offset(),
                    preferred,
                );
                return vec![offense.with_correction(correction)];
            }
        }

        vec![]
    }
}
