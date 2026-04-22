//! Lint/ErbNewArguments - ERB.new positional args deprecated since Ruby 2.6.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/erb_new_arguments.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct ErbNewArguments;

impl ErbNewArguments {
    pub fn new() -> Self {
        Self
    }
}

struct ErbNewArgumentsVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl ErbNewArgumentsVisitor<'_> {
    fn is_erb_receiver(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let c = node.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(c.name().as_slice());
                name == "ERB"
            }
            Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                if cp.parent().is_some() { return false; }
                let const_id = match cp.name() { Some(id) => id, None => return false };
                let name = String::from_utf8_lossy(const_id.as_slice());
                name == "ERB"
            }
            _ => false,
        }
    }

    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        if !self.ctx.ruby_version_at_least(2, 6) {
            return;
        }

        let method = node_name!(node);
        if method != "new" {
            return;
        }

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };
        if !Self::is_erb_receiver(&receiver) {
            return;
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };

        let all_args: Vec<Node> = args.arguments().iter().collect();
        if all_args.len() < 2 {
            return;
        }

        let src = self.ctx.source;

        // Separate positional from keyword args
        let mut pos_args: Vec<usize> = Vec::new(); // indices into all_args
        let mut kw_arg_indices: Vec<usize> = Vec::new();

        for (i, arg) in all_args.iter().enumerate() {
            if matches!(arg, Node::KeywordHashNode { .. }) {
                kw_arg_indices.push(i);
            } else {
                pos_args.push(i);
            }
        }

        if pos_args.len() < 2 {
            return;
        }

        let mut new_offenses: Vec<Offense> = Vec::new();

        // eoutvar (4th positional = pos_args[3])
        if pos_args.len() >= 4 {
            let idx = pos_args[3];
            let node_ref = &all_args[idx];
            let loc = node_ref.location();
            let ev_src = &src[loc.start_offset()..loc.end_offset()];
            let msg = format!(
                "Passing eoutvar with the 4th argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, eoutvar: {})` instead.",
                ev_src
            );
            new_offenses.push(self.ctx.offense_with_range(
                "Lint/ErbNewArguments", &msg, Severity::Warning, loc.start_offset(), loc.end_offset(),
            ));
        }

        // trim_mode (3rd positional = pos_args[2])
        if pos_args.len() >= 3 {
            let idx = pos_args[2];
            let node_ref = &all_args[idx];
            let loc = node_ref.location();
            let tm_src = &src[loc.start_offset()..loc.end_offset()];
            let msg = format!(
                "Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: {})` instead.",
                tm_src
            );
            new_offenses.push(self.ctx.offense_with_range(
                "Lint/ErbNewArguments", &msg, Severity::Warning, loc.start_offset(), loc.end_offset(),
            ));
        }

        // safe_level (2nd positional = pos_args[1])
        {
            let idx = pos_args[1];
            let node_ref = &all_args[idx];
            let loc = node_ref.location();
            let msg = "Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.";
            new_offenses.push(self.ctx.offense_with_range(
                "Lint/ErbNewArguments", msg, Severity::Warning, loc.start_offset(), loc.end_offset(),
            ));
        }

        // Build correction
        let first_loc = all_args[pos_args[0]].location();
        let first_src = &src[first_loc.start_offset()..first_loc.end_offset()];
        let mut kw_parts: Vec<String> = Vec::new();

        if pos_args.len() >= 3 {
            let idx = pos_args[2];
            let loc = all_args[idx].location();
            let tm_src = &src[loc.start_offset()..loc.end_offset()];
            kw_parts.push(format!("trim_mode: {}", tm_src));
        }
        if pos_args.len() >= 4 {
            let idx = pos_args[3];
            let loc = all_args[idx].location();
            let ev_src = &src[loc.start_offset()..loc.end_offset()];
            kw_parts.push(format!("eoutvar: {}", ev_src));
        }

        // Existing keyword args: extract individual pairs from KeywordHashNode
        // Filter out pairs that conflict with positional-converted keywords
        for &ki in &kw_arg_indices {
            let kw_node = &all_args[ki];
            if let Node::KeywordHashNode { .. } = kw_node {
                let kh = kw_node.as_keyword_hash_node().unwrap();
                for elem in kh.elements().iter() {
                    let elem_src_loc = elem.location();
                    let elem_src = &src[elem_src_loc.start_offset()..elem_src_loc.end_offset()];
                    let conflicts_trim = elem_src.starts_with("trim_mode:") && pos_args.len() >= 3;
                    let conflicts_eout = elem_src.starts_with("eoutvar:") && pos_args.len() >= 4;
                    if !conflicts_trim && !conflicts_eout {
                        kw_parts.push(elem_src.to_string());
                    }
                }
            }
        }

        let new_args_str = if kw_parts.is_empty() {
            first_src.to_string()
        } else {
            format!("{}, {}", first_src, kw_parts.join(", "))
        };

        let args_start = all_args.first().unwrap().location().start_offset();
        let args_end = all_args.last().unwrap().location().end_offset();
        let correction = Correction::replace(args_start, args_end, &new_args_str);

        // Attach correction to last offense (safe_level)
        let last_idx = new_offenses.len() - 1;
        let last_offense = new_offenses.remove(last_idx);
        new_offenses.push(last_offense.with_correction(correction));

        self.offenses.extend(new_offenses);
    }
}

impl Visit<'_> for ErbNewArgumentsVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for ErbNewArguments {
    fn name(&self) -> &'static str {
        "Lint/ErbNewArguments"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ErbNewArgumentsVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/ErbNewArguments", |_cfg| {
    Some(Box::new(ErbNewArguments::new()))
});
