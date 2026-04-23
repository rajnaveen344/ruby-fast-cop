use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const STD_STREAMS: &[&str] = &["STDOUT", "STDERR", "STDIN"];

#[derive(Default)]
pub struct GlobalStdStream;

impl GlobalStdStream {
    pub fn new() -> Self {
        Self
    }

    fn gvar_name(const_name: &str) -> String {
        format!("${}", const_name.to_lowercase())
    }

    /// Check if source before `start` indicates this is `$gvar = CONST` assignment
    fn is_gvar_assignment(const_name: &str, start: usize, source: &str) -> bool {
        let before = source[..start].trim_end();
        if !before.ends_with('=') {
            return false;
        }
        let before_eq = source[..before.len() - 1].trim_end();
        let expected_gvar = format!("${}", const_name.to_lowercase());
        before_eq.ends_with(&expected_gvar)
    }
}

impl Cop for GlobalStdStream {
    fn name(&self) -> &'static str {
        "Style/GlobalStdStream"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_constant_read(
        &self,
        node: &ruby_prism::ConstantReadNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let name = String::from_utf8_lossy(node.name().as_slice());
        if !STD_STREAMS.contains(&name.as_ref()) {
            return vec![];
        }
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        // Check if preceded by `::` (it would be a ConstantPathNode child, so skip here)
        if start >= 2 {
            let bytes = ctx.source.as_bytes();
            if bytes[start - 2] == b':' && bytes[start - 1] == b':' {
                // This ConstantReadNode is the leaf of ::STDOUT — handled by check_constant_path
                return vec![];
            }
        }

        // Skip `$gvar = CONST` pattern
        if Self::is_gvar_assignment(&name, start, ctx.source) {
            return vec![];
        }

        let gvar = Self::gvar_name(&name);
        let msg = format!("Use `{}` instead of `{}`.", gvar, name);
        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)]
    }

    fn check_constant_path(
        &self,
        node: &ruby_prism::ConstantPathNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        // Handle ::STDOUT, ::STDERR, ::STDIN (cbase-rooted, no parent namespace)
        if node.parent().is_some() {
            // Has a parent namespace like Foo::STDOUT — skip
            return vec![];
        }
        // Get the constant name
        let name_id = match node.name() {
            Some(id) => id,
            None => return vec![],
        };
        let name = String::from_utf8_lossy(name_id.as_slice());
        if !STD_STREAMS.contains(&name.as_ref()) {
            return vec![];
        }
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        // Skip `$gvar = ::CONST` pattern
        if Self::is_gvar_assignment(&name, start, ctx.source) {
            return vec![];
        }

        let gvar = Self::gvar_name(&name);
        let msg = format!("Use `{}` instead of `{}`.", gvar, name);
        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)]
    }
}

crate::register_cop!("Style/GlobalStdStream", |_cfg| {
    Some(Box::new(GlobalStdStream::new()))
});
