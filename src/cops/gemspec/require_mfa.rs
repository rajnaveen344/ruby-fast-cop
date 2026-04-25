//! Gemspec/RequireMFA cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "`metadata['rubygems_mfa_required']` must be set to `'true'`.";

#[derive(Default)]
pub struct RequireMFA;

impl RequireMFA {
    pub fn new() -> Self { Self }
}

impl Cop for RequireMFA {
    fn name(&self) -> &'static str { "Gemspec/RequireMFA" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut tf = TopFinder { ctx, out: vec![] };
        tf.visit(&tree);
        tf.out
    }
}

struct TopFinder<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    out: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for TopFinder<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if !self.out.is_empty() { return; }
        if is_gem_spec_new(node) {
            if let Some(block) = node.block() {
                if let Some(bn) = block.as_block_node() {
                    self.process_block(node, &bn);
                    return;
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a, 'b> TopFinder<'a, 'b> {
    fn process_block(&mut self, call: &ruby_prism::CallNode, bn: &ruby_prism::BlockNode) {
        let block_var = extract_block_var(bn).unwrap_or_else(|| "spec".to_string());

        let mut ana = BodyAnalyzer {
            block_var: &block_var,
            source: self.ctx.source,
            meta_value: None,
            all_assign_ends: vec![],
        };
        if let Some(body) = bn.body() {
            ana.visit(&body);
        }

        match compute_mfa(&ana.meta_value) {
            MfaInfo::Ok => {}
            MfaInfo::Wrong { start, end } => {
                let off = self.ctx.offense_with_range(
                    "Gemspec/RequireMFA", MSG, Severity::Convention, start, end,
                ).with_correction(Correction::replace(start, end, "'true'".to_string()));
                self.out.push(off);
            }
            MfaInfo::Missing => {
                let loc = call.location();
                let (s, e) = first_line_range(self.ctx.source, loc.start_offset());
                let mut off = self.ctx.offense_with_range(
                    "Gemspec/RequireMFA", MSG, Severity::Convention, s, e,
                );
                if let Some(corr) = build_missing_correction(&ana, &block_var, bn) {
                    off = off.with_correction(corr);
                }
                self.out.push(off);
            }
        }
    }
}

enum MfaInfo {
    Ok,
    Wrong { start: usize, end: usize },
    Missing,
}

fn compute_mfa(mv: &Option<MetaValue>) -> MfaInfo {
    match mv {
        None => MfaInfo::Missing,
        Some(mv) => match &mv.kind {
            ValueKind::Str { is_true } => {
                if *is_true { MfaInfo::Ok }
                else { MfaInfo::Wrong { start: mv.val_start, end: mv.val_end } }
            }
            ValueKind::Hash { mfa, .. } => match mfa {
                None => MfaInfo::Missing,
                Some((s, e, true)) => { let _ = (s, e); MfaInfo::Ok }
                Some((s, e, false)) => MfaInfo::Wrong { start: *s, end: *e },
            },
            ValueKind::Other => MfaInfo::Missing,
        },
    }
}

fn build_missing_correction(
    ana: &BodyAnalyzer,
    block_var: &str,
    bn: &ruby_prism::BlockNode,
) -> Option<Correction> {
    if let Some(mv) = &ana.meta_value {
        match &mv.kind {
            ValueKind::Hash { close_brace_start, last_pair_end, .. } => {
                if let Some(lpe) = last_pair_end {
                    return Some(Correction::insert(
                        *lpe,
                        ",\n'rubygems_mfa_required' => 'true'".to_string(),
                    ));
                } else {
                    return Some(Correction::insert(
                        *close_brace_start,
                        "'rubygems_mfa_required' => 'true'".to_string(),
                    ));
                }
            }
            ValueKind::Other | ValueKind::Str { .. } => return None,
        }
    }
    let directive = format!("{}.metadata['rubygems_mfa_required'] = 'true'", block_var);
    if let Some(&(_s, e)) = ana.all_assign_ends.last() {
        return Some(Correction::insert(e, format!("\n{}", directive)));
    }
    let closing = bn.closing_loc();
    Some(Correction::insert(closing.start_offset(), format!("{}\n", directive)))
}

struct MetaValue {
    val_start: usize,
    val_end: usize,
    kind: ValueKind,
}

enum ValueKind {
    Str { is_true: bool },
    Hash {
        close_brace_start: usize,
        last_pair_end: Option<usize>,
        mfa: Option<(usize, usize, bool)>,
    },
    Other,
}

struct BodyAnalyzer<'a> {
    block_var: &'a str,
    source: &'a str,
    meta_value: Option<MetaValue>,
    all_assign_ends: Vec<(usize, usize)>,
}

impl<'a> Visit<'_> for BodyAnalyzer<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name = node_name!(node);
        // direct: spec.metadata = <val>
        if name.as_ref() == "metadata=" {
            if let Some(recv) = node.receiver() {
                if is_block_var(&recv, self.block_var) {
                    if let Some(args) = node.arguments() {
                        if let Some(val) = args.arguments().iter().next() {
                            let loc = node.location();
                            self.all_assign_ends.push((loc.start_offset(), loc.end_offset()));
                            if self.meta_value.is_none() {
                                let vloc = val.location();
                                let kind = analyze_value_kind(&val);
                                self.meta_value = Some(MetaValue {
                                    val_start: vloc.start_offset(),
                                    val_end: vloc.end_offset(),
                                    kind,
                                });
                            }
                        }
                    }
                }
            }
        }
        // indexed: spec.metadata['key'] = <val>
        if name.as_ref() == "[]=" {
            if let Some(recv) = node.receiver() {
                if is_spec_metadata_call(&recv, self.block_var) {
                    if let Some(args) = node.arguments() {
                        let argv: Vec<_> = args.arguments().iter().collect();
                        if argv.len() == 2 {
                            if let Some(key_str) = argv[0].as_string_node() {
                                let key_text = String::from_utf8_lossy(key_str.unescaped()).to_string();
                                let loc = node.location();
                                self.all_assign_ends.push((loc.start_offset(), loc.end_offset()));
                                if self.meta_value.is_none() && key_text == "rubygems_mfa_required" {
                                    let val = &argv[1];
                                    let vloc = val.location();
                                    let is_true_str = val.as_string_node()
                                        .map(|s| String::from_utf8_lossy(s.unescaped()) == "true")
                                        .unwrap_or(false);
                                    self.meta_value = Some(MetaValue {
                                        val_start: vloc.start_offset(),
                                        val_end: vloc.end_offset(),
                                        kind: ValueKind::Str { is_true: is_true_str },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn analyze_value_kind(val: &Node) -> ValueKind {
    if let Some(s) = val.as_string_node() {
        let text = String::from_utf8_lossy(s.unescaped()).to_string();
        return ValueKind::Str { is_true: text == "true" };
    }
    if let Some(h) = val.as_hash_node() {
        let close_brace_start = h.closing_loc().start_offset();
        let mut last_pair_end: Option<usize> = None;
        let mut mfa: Option<(usize, usize, bool)> = None;
        for el in h.elements().iter() {
            let eloc = el.location();
            last_pair_end = Some(eloc.end_offset());
            if let Some(pair) = el.as_assoc_node() {
                let key = pair.key();
                if let Some(ks) = key.as_string_node() {
                    let key_text = String::from_utf8_lossy(ks.unescaped()).to_string();
                    if key_text == "rubygems_mfa_required" && mfa.is_none() {
                        let vnode = pair.value();
                        let vloc = vnode.location();
                        let is_true = vnode.as_string_node()
                            .map(|s| String::from_utf8_lossy(s.unescaped()) == "true")
                            .unwrap_or(false);
                        mfa = Some((vloc.start_offset(), vloc.end_offset(), is_true));
                    }
                }
            }
        }
        return ValueKind::Hash { close_brace_start, last_pair_end, mfa };
    }
    ValueKind::Other
}

fn is_block_var(node: &Node, var: &str) -> bool {
    if let Some(l) = node.as_local_variable_read_node() {
        return String::from_utf8_lossy(l.name().as_slice()) == var;
    }
    false
}

fn is_spec_metadata_call(node: &Node, var: &str) -> bool {
    let call = match node.as_call_node() { Some(c) => c, None => return false };
    let name = String::from_utf8_lossy(match call.name() { n => n.as_slice() }).to_string();
    if name != "metadata" { return false; }
    let recv = match call.receiver() { Some(r) => r, None => return false };
    is_block_var(&recv, var)
}

fn first_line_range(source: &str, start: usize) -> (usize, usize) {
    let bytes = source.as_bytes();
    let mut e = start;
    while e < bytes.len() && bytes[e] != b'\n' { e += 1; }
    (start, e)
}

fn is_gem_spec_new(node: &ruby_prism::CallNode) -> bool {
    if node_name!(node).as_ref() != "new" { return false; }
    let recv = match node.receiver() { Some(r) => r, None => return false };
    let cp = match recv.as_constant_path_node() { Some(c) => c, None => return false };
    let name = match cp.name() { Some(n) => n, None => return false };
    if String::from_utf8_lossy(name.as_slice()) != "Specification" { return false; }
    let parent = match cp.parent() { Some(p) => p, None => return false };
    if let Some(pr) = parent.as_constant_read_node() {
        return String::from_utf8_lossy(pr.name().as_slice()) == "Gem";
    }
    false
}

fn extract_block_var(bn: &ruby_prism::BlockNode) -> Option<String> {
    let params = bn.parameters()?;
    let bp = params.as_block_parameters_node()?;
    let p = bp.parameters()?;
    let first = p.requireds().iter().next()?;
    let rp = first.as_required_parameter_node()?;
    Some(String::from_utf8_lossy(rp.name().as_slice()).to_string())
}

crate::register_cop!("Gemspec/RequireMFA", |_cfg| Some(Box::new(RequireMFA::new())));
