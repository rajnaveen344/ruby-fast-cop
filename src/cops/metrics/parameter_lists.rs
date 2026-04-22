//! Metrics/ParameterLists cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::code_length::find_end_of_first_line;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct ParameterLists {
    max: usize,
    max_optional: usize,
    count_keyword_args: bool,
}

impl ParameterLists {
    pub fn new(max: usize, max_optional: usize, count_keyword_args: bool) -> Self {
        Self { max, max_optional, count_keyword_args }
    }

    fn count_params(&self, params: &ruby_prism::ParametersNode) -> usize {
        let mut count = 0;
        count += params.requireds().len();
        count += params.optionals().len();
        // Splat (*args) — rest() returns Option<Node>
        if let Some(rest) = params.rest() {
            // SplatParameterNode = explicit splat; implicit rest is different
            if matches!(rest, ruby_prism::Node::RestParameterNode { .. }) {
                count += 1;
            }
        }
        count += params.posts().len();
        if self.count_keyword_args {
            count += params.keywords().len();
            // keyword_rest (**kwargs) — only count if it's a named rest (not **nil)
            if let Some(kw_rest) = params.keyword_rest() {
                if matches!(kw_rest, ruby_prism::Node::KeywordRestParameterNode { .. }) {
                    count += 1;
                }
            }
        }
        // block arg (&block) NOT counted
        count
    }

    fn count_optionals(params: &ruby_prism::ParametersNode) -> usize {
        params.optionals().len()
    }

    /// Check if def node is `initialize` inside a Struct.new or Data.define block
    fn is_in_struct_or_data_block(node: &ruby_prism::DefNode, ctx: &CheckContext) -> bool {
        if node_name!(node) != "initialize" {
            return false;
        }
        is_initialize_in_struct_block(ctx.source, node.location().start_offset())
    }
}

/// Check if a `def initialize` at `def_offset` is inside a Struct.new or Data.define block
fn is_initialize_in_struct_block(source: &str, def_offset: usize) -> bool {
    let result = ruby_prism::parse(source.as_bytes());
    let tree = result.node();
    let mut checker = StructBlockChecker { def_offset, found: false };
    checker.visit(&tree);
    checker.found
}

struct StructBlockChecker {
    def_offset: usize,
    found: bool,
}

impl ruby_prism::Visit<'_> for StructBlockChecker {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check if this call is Struct.new or Data.define with a block
        if is_struct_or_data_call(node) {
            if let Some(block_node) = node.block() {
                if let ruby_prism::Node::BlockNode { .. } = block_node {
                    let block = block_node.as_block_node().unwrap();
                    let bloc_start = block.location().start_offset();
                    let bloc_end = block.location().end_offset();
                    if self.def_offset >= bloc_start && self.def_offset <= bloc_end {
                        self.found = true;
                        return;
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn is_struct_or_data_call(node: &ruby_prism::CallNode) -> bool {
    let method = node_name!(node);
    if method != "new" && method != "define" {
        return false;
    }
    let recv = match node.receiver() {
        Some(r) => r,
        None => return false,
    };
    let recv_name = match &recv {
        ruby_prism::Node::ConstantReadNode { .. } => {
            node_name!(recv.as_constant_read_node().unwrap()).to_string()
        }
        ruby_prism::Node::ConstantPathNode { .. } => {
            let cp = recv.as_constant_path_node().unwrap();
            if cp.parent().is_some() { return false; }
            match cp.name() {
                Some(id) => String::from_utf8_lossy(id.as_slice()).to_string(),
                None => return false,
            }
        }
        _ => return false,
    };
    (recv_name == "Struct" && method == "new") || (recv_name == "Data" && method == "define")
}

impl Default for ParameterLists {
    fn default() -> Self { Self::new(5, 3, true) }
}

impl Cop for ParameterLists {
    fn name(&self) -> &'static str { "Metrics/ParameterLists" }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        // DefNode.parameters() returns Option<ParametersNode> directly
        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![],
        };

        let mut offenses = Vec::new();

        let skip_total = Self::is_in_struct_or_data_block(node, ctx);

        // Check optional parameter count → offense on entire def line
        let opt_count = Self::count_optionals(&params);
        if opt_count > self.max_optional {
            let msg = format!(
                "Method has too many optional parameters. [{}/{}]",
                opt_count, self.max_optional
            );
            let def_start = node.location().start_offset();
            let def_end = find_end_of_first_line(ctx.source, def_start);
            offenses.push(ctx.offense_with_range(self.name(), &msg, self.severity(), def_start, def_end));
        }

        if !skip_total {
            let total = self.count_params(&params);
            if total > self.max {
                let msg = format!(
                    "Avoid parameter lists longer than {} parameters. [{}/{}]",
                    self.max, total, self.max
                );
                // Offense spans lparen.start..rparen.end to match RuboCop's ArgsNode range
                let start = node.lparen_loc().map(|l| l.start_offset()).unwrap_or_else(|| params.location().start_offset());
                let end = node.rparen_loc().map(|l| l.end_offset()).unwrap_or_else(|| params.location().end_offset());
                offenses.push(ctx.offense_with_range(
                    self.name(), &msg, self.severity(), start, end,
                ));
            }
        }

        offenses
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct ParameterListsCfg {
    max: usize,
    max_optional_parameters: usize,
    count_keyword_args: bool,
}

impl Default for ParameterListsCfg {
    fn default() -> Self { Self { max: 5, max_optional_parameters: 3, count_keyword_args: true } }
}

crate::register_cop!("Metrics/ParameterLists", |cfg| {
    let c: ParameterListsCfg = cfg.typed("Metrics/ParameterLists");
    Some(Box::new(ParameterLists::new(c.max, c.max_optional_parameters, c.count_keyword_args)))
});
