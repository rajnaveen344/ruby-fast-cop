//! Style/StderrPuts cop
//!
//! Flags `$stderr.puts(...)` and `STDERR.puts(...)` — prefer `warn`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

const MSG: &str = "Use `warn` instead of `%s.puts` to allow such output to be disabled.";

#[derive(Default)]
pub struct StderrPuts;

impl StderrPuts {
    pub fn new() -> Self {
        Self
    }

    fn receiver_name(node: &ruby_prism::CallNode, source: &str) -> Option<String> {
        let recv = node.receiver()?;
        let src = &source[recv.location().start_offset()..recv.location().end_offset()];
        Some(src.to_string())
    }
}

impl Cop for StderrPuts {
    fn name(&self) -> &'static str {
        "Style/StderrPuts"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "puts" {
            return vec![];
        }

        let recv_src = match Self::receiver_name(node, ctx.source) {
            Some(s) => s,
            None => return vec![],
        };

        // Must be $stderr or STDERR (with optional :: prefix)
        let is_stderr = recv_src == "$stderr"
            || recv_src == "STDERR"
            || recv_src == "::STDERR";
        if !is_stderr {
            return vec![];
        }

        // Must have at least 1 argument
        let has_args = node.arguments().map_or(false, |a| !a.arguments().is_empty());
        if !has_args {
            return vec![];
        }

        let recv = node.receiver().unwrap();
        let recv_start = recv.location().start_offset();
        let recv_end = recv.location().end_offset();
        let msg = MSG.replacen("%s", &recv_src, 1);

        // Offense: from receiver start to method name end (just `$stderr.puts` / `STDERR.puts`)
        let method_loc = node.message_loc().unwrap_or_else(|| node.location());
        let offense_end = method_loc.end_offset();

        // Correction: replace `$stderr.puts(args)` with `warn(args)`
        // Keep the args, replace `recv.puts` → `warn`
        let call_end = node.location().end_offset();
        let args_src = if let Some(a) = node.arguments() {
            let args_start = a.location().start_offset();
            let args_end = a.location().end_offset();
            &ctx.source[args_start..args_end]
        } else {
            ""
        };

        // Check if original has parens
        let method_end = method_loc.end_offset();
        let has_parens = ctx.source[method_end..call_end].trim_start().starts_with('(');

        let corrected = if has_parens {
            format!("warn({})", args_src)
        } else {
            format!("warn({})", args_src)
        };

        let correction = Correction::replace(recv_start, call_end, corrected);

        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), recv_start, offense_end)
            .with_correction(correction)]
    }
}

crate::register_cop!("Style/StderrPuts", |_cfg| {
    Some(Box::new(StderrPuts::new()))
});
