//! Style/ItBlockParameter
//!
//! Enforces use of the `it` block parameter (Ruby 3.4+). Four styles:
//! `allow_single_line` (default), `only_numbered_parameters`, `always`,
//! `disallow`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{BlockNode, Node, Visit};

const MSG_USE_IT: &str = "Use `it` block parameter.";
const MSG_AVOID_IT: &str = "Avoid using `it` block parameter.";
const MSG_AVOID_IT_MULTILINE: &str = "Avoid using `it` block parameter for multi-line blocks.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItBlockParameterStyle {
    AllowSingleLine,
    OnlyNumberedParameters,
    Always,
    Disallow,
}

pub struct ItBlockParameter {
    style: ItBlockParameterStyle,
}

impl ItBlockParameter {
    pub fn new() -> Self { Self { style: ItBlockParameterStyle::AllowSingleLine } }
    pub fn with_style(style: ItBlockParameterStyle) -> Self { Self { style } }
}

impl Default for ItBlockParameter {
    fn default() -> Self { Self::new() }
}

/// Find lvar reads in body whose source text matches `target`.
fn find_lvar_reads_matching<'a>(body: &Node<'a>, source: &'a str, target: &str) -> Vec<(usize, usize)> {
    struct V<'a> {
        source: &'a str,
        target: String,
        hits: Vec<(usize, usize)>,
    }
    impl<'a> Visit<'_> for V<'a> {
        fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
            let loc = node.location();
            let text = &self.source[loc.start_offset()..loc.end_offset()];
            if text == self.target {
                self.hits.push((loc.start_offset(), loc.end_offset()));
            }
        }
        fn visit_it_local_variable_read_node(&mut self, node: &ruby_prism::ItLocalVariableReadNode) {
            if self.target == "it" {
                let loc = node.location();
                self.hits.push((loc.start_offset(), loc.end_offset()));
            }
        }
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
            // `it` inside an it-block is a CallNode with name=="it", no args, no receiver.
            if self.target == "it"
                && node.name().as_slice() == b"it"
                && node.receiver().is_none()
                && node.arguments().is_none()
                && node.opening_loc().is_none()
                && node.block().is_none()
            {
                let loc = node.message_loc().unwrap_or(node.location());
                self.hits.push((loc.start_offset(), loc.end_offset()));
            }
            ruby_prism::visit_call_node(self, node);
        }
    }
    let mut v = V { source, target: target.to_string(), hits: Vec::new() };
    v.visit(body);
    v.hits
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    style: ItBlockParameterStyle,
    offenses: Vec<Offense>,
    call_stack: Vec<usize>, // start_offset of enclosing CallNode (for block offenses)
}

impl<'a> V<'a> {
    fn push(&mut self, start: usize, end: usize, msg: &str) {
        self.offenses.push(self.ctx.offense_with_range(
            "Style/ItBlockParameter", msg, Severity::Convention, start, end,
        ));
    }
}

impl<'a> Visit<'_> for V<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.call_stack.push(node.location().start_offset());
        ruby_prism::visit_call_node(self, node);
        self.call_stack.pop();
    }
    fn visit_block_node(&mut self, node: &BlockNode) {
        self.process(node);
        ruby_prism::visit_block_node(self, node);
    }
}

impl<'a> V<'a> {
    fn process(&mut self, node: &BlockNode) {
        let params = node.parameters();
        let Some(body) = node.body() else {
            // Recurse handled by outer visitor.
            return;
        };

        match (self.style, &params) {
            // Named single arg, style=always → convert to `it`
            (ItBlockParameterStyle::Always, Some(p)) => {
                if let Some(bp) = p.as_block_parameters_node() {
                    if let Some(params_node) = bp.parameters() {
                        let requireds: Vec<_> = params_node.requireds().iter().collect();
                        let optionals: Vec<_> = params_node.optionals().iter().collect();
                        let rest = params_node.rest().is_some();
                        let kw: Vec<_> = params_node.keywords().iter().collect();
                        let kwrest = params_node.keyword_rest().is_some();
                        let block = params_node.block().is_some();
                        let total = requireds.len() + optionals.len() + kw.len()
                            + (if rest { 1 } else { 0 })
                            + (if kwrest { 1 } else { 0 })
                            + (if block { 1 } else { 0 });
                        if total == 1 && requireds.len() == 1 {
                            let only = &requireds[0];
                            if let Some(req) = only.as_required_parameter_node() {
                                let name = String::from_utf8_lossy(req.name().as_slice()).into_owned();
                                for (s, e) in find_lvar_reads_matching(&body, self.ctx.source, &name) {
                                    self.push(s, e, MSG_USE_IT);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Numbered params → `it`
        if matches!(
            self.style,
            ItBlockParameterStyle::AllowSingleLine
                | ItBlockParameterStyle::OnlyNumberedParameters
                | ItBlockParameterStyle::Always
        ) {
            if let Some(p) = &params {
                if let Some(np) = p.as_numbered_parameters_node() {
                    if np.maximum() == 1 {
                        for (s, e) in find_lvar_reads_matching(&body, self.ctx.source, "_1") {
                            self.push(s, e, MSG_USE_IT);
                        }
                    }
                }
            }
        }

        // it-block handling
        if let Some(p) = &params {
            if p.as_it_parameters_node().is_some() {
                match self.style {
                    ItBlockParameterStyle::AllowSingleLine => {
                        let loc = node.location();
                        if !self.ctx.same_line(loc.start_offset(), loc.end_offset()) {
                            // Offense range = whole block's parent call — use outer
                            // call via body-start is hard. RuboCop adds offense on
                            // the itblock which includes call + block. We use node.loc
                            // which is the block only — but fixture column_start=0,
                            // column_end=8 for `block do\n ...`. So offense is on the
                            // call + block. Use from block start back to line-start non-ws.
                            self.push_multiline_outer(node);
                        }
                    }
                    ItBlockParameterStyle::Disallow => {
                        for (s, e) in find_lvar_reads_matching(&body, self.ctx.source, "it") {
                            self.push(s, e, MSG_AVOID_IT);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn push_multiline_outer(&mut self, block: &BlockNode) {
        let end = block.location().end_offset();
        let call_start = self.call_stack.last().copied()
            .unwrap_or(block.location().start_offset());
        self.push(call_start, end, MSG_AVOID_IT_MULTILINE);
    }
}

impl Cop for ItBlockParameter {
    fn name(&self) -> &'static str { "Style/ItBlockParameter" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 4) { return vec![] }
        let mut v = V { ctx, style: self.style, offenses: Vec::new(), call_stack: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/ItBlockParameter", |cfg| {
    let style = cfg.get_cop_config("Style/ItBlockParameter")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "only_numbered_parameters" => ItBlockParameterStyle::OnlyNumberedParameters,
            "always" => ItBlockParameterStyle::Always,
            "disallow" => ItBlockParameterStyle::Disallow,
            _ => ItBlockParameterStyle::AllowSingleLine,
        })
        .unwrap_or(ItBlockParameterStyle::AllowSingleLine);
    Some(Box::new(ItBlockParameter::with_style(style)))
});
