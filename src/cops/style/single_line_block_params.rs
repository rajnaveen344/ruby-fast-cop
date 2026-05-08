//! Style/SingleLineBlockParams - Block parameters of single-line method calls
//! must match configured names.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/single_line_block_params.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/SingleLineBlockParams";

/// (method_name, expected_param_names)
#[derive(Default)]
pub struct SingleLineBlockParams {
    methods: Vec<(String, Vec<String>)>,
}

impl SingleLineBlockParams {
    pub fn new() -> Self {
        Self { methods: Vec::new() }
    }

    pub fn with_methods(methods: Vec<(String, Vec<String>)>) -> Self {
        Self { methods }
    }

    fn target_args(&self, method_name: &str) -> Option<&[String]> {
        self.methods
            .iter()
            .find(|(m, _)| m == method_name)
            .map(|(_, v)| v.as_slice())
    }
}

impl Cop for SingleLineBlockParams {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, call: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Need a configured method (and a receiver — `eligible_method?` requires it)
        if call.receiver().is_none() {
            return vec![];
        }
        let method_name = String::from_utf8_lossy(call.name().as_slice()).to_string();
        let target = match self.target_args(&method_name) {
            Some(t) => t,
            None => return vec![],
        };

        // Block must exist and be a BlockNode
        let block_node = match call.block() {
            Some(b) => b,
            None => return vec![],
        };
        let block = match block_node.as_block_node() {
            Some(b) => b,
            None => return vec![],
        };

        // single-line check
        let open_loc = block.opening_loc();
        let close_loc = block.closing_loc();
        if !ctx.same_line(open_loc.start_offset(), close_loc.end_offset()) {
            return vec![];
        }

        // arguments? && all required arg type
        let params_node = match block.parameters() {
            Some(p) => p,
            None => return vec![],
        };
        let bp = match params_node.as_block_parameters_node() {
            Some(bp) => bp,
            None => return vec![],
        };

        // Need at least one parameter; all must be RequiredParameterNode (no destructuring,
        // no optionals, no rest, no kw, no block).
        let pn = match bp.parameters() {
            Some(p) => p,
            None => return vec![],
        };
        if pn.optionals().iter().count() > 0
            || pn.rest().is_some()
            || pn.keywords().iter().count() > 0
            || pn.keyword_rest().is_some()
            || pn.block().is_some()
        {
            return vec![];
        }
        let requireds: Vec<Node<'_>> = pn.requireds().iter().collect();
        if requireds.is_empty() {
            return vec![];
        }
        for r in &requireds {
            if !matches!(r, Node::RequiredParameterNode { .. }) {
                return vec![];
            }
        }

        // Collect actual arg names
        let actual: Vec<String> = requireds
            .iter()
            .map(|r| {
                let rpn = r.as_required_parameter_node().unwrap();
                String::from_utf8_lossy(rpn.name().as_slice()).to_string()
            })
            .collect();

        // args_match?: strip leading underscores, compare to first(N) of target
        let actual_no_underscores: Vec<String> = actual
            .iter()
            .map(|a| a.trim_start_matches('_').to_string())
            .collect();
        let n = actual_no_underscores.len();
        let expected_prefix: Vec<String> = target.iter().take(n).cloned().collect();
        if actual_no_underscores == expected_prefix {
            return vec![];
        }

        // Build preferred args, preserving leading underscore from current arg
        let preferred: Vec<String> = actual
            .iter()
            .enumerate()
            .map(|(i, current)| {
                let pref = &target[i.min(target.len() - 1)];
                if current.starts_with('_') {
                    format!("_{}", pref)
                } else {
                    pref.clone()
                }
            })
            .collect();
        let joined = preferred.join(", ");

        // Offense range = block parameters node (the `|...|`)
        let (po_start, pc_end) = match (bp.opening_loc(), bp.closing_loc()) {
            (Some(o), Some(c)) => (o.start_offset(), c.end_offset()),
            _ => return vec![],
        };

        let message = format!(
            "Name `{}` block params `|{}|`.",
            method_name, joined
        );

        // Build correction: replace |old_params| with |new_params|, rename lvar uses in body
        let name_map: Vec<(String, String)> = actual.iter().zip(preferred.iter())
            .filter(|(a, p)| a != p)
            .map(|(a, p)| (a.clone(), p.clone()))
            .collect();

        let mut edits: Vec<Edit> = Vec::new();
        // Edit 1: replace params
        edits.push(Edit {
            start_offset: po_start,
            end_offset: pc_end,
            replacement: format!("|{}|", joined),
        });

        // Edit 2+: replace lvar reads in body
        if let Some(body) = block.body() {
            let mut lvar_visitor = LvarVisitor {
                name_map: &name_map,
                edits: &mut edits,
            };
            lvar_visitor.visit(&body);
        }

        let correction = Correction { edits };
        vec![ctx.offense_with_range(COP_NAME, &message, Severity::Convention, po_start, pc_end)
            .with_correction(correction)]
    }
}

struct LvarVisitor<'a> {
    name_map: &'a Vec<(String, String)>,
    edits: &'a mut Vec<Edit>,
}

impl<'a> Visit<'_> for LvarVisitor<'a> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if let Some((_, new_name)) = self.name_map.iter().find(|(old, _)| old == &name) {
            self.edits.push(Edit {
                start_offset: node.location().start_offset(),
                end_offset: node.location().end_offset(),
                replacement: new_name.clone(),
            });
        }
        ruby_prism::visit_local_variable_read_node(self, node);
    }
}

crate::register_cop!("Style/SingleLineBlockParams", |cfg| {
    let cop_config = cfg.get_cop_config("Style/SingleLineBlockParams");
    let methods: Vec<(String, Vec<String>)> = cop_config
        .and_then(|c| c.raw.get("Methods"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            let mut out: Vec<(String, Vec<String>)> = Vec::new();
            for item in seq {
                if let Some(map) = item.as_mapping() {
                    if let Some((k, v)) = map.iter().next() {
                        let key = k.as_str().map(String::from);
                        let names: Option<Vec<String>> = v
                            .as_sequence()
                            .map(|s| s.iter().filter_map(|x| x.as_str().map(String::from)).collect());
                        if let (Some(k), Some(names)) = (key, names) {
                            out.push((k, names));
                        }
                    }
                }
            }
            out
        })
        .unwrap_or_default();
    Some(Box::new(SingleLineBlockParams::with_methods(methods)))
});
