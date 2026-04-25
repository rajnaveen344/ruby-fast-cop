//! Style/SuperArguments — flag `super(args)` when args identical to enclosing
//! method's parameters; suggest bare `super`.
//!
//! Ported from `lib/rubocop/cop/style/super_arguments.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{
    DefNode, Node, ParametersNode, SuperNode, Visit,
};

const MSG: &str = "Call `super` without arguments and parentheses when the signature is identical.";
const MSG_INLINE_BLOCK: &str =
    "Call `super` without arguments and parentheses when all positional and keyword arguments are forwarded.";

#[derive(Default)]
pub struct SuperArguments;

impl SuperArguments {
    pub fn new() -> Self { Self }
}

// ── Param descriptors (mirrors RuboCop's argument_list) ──

#[derive(Debug, Clone, PartialEq, Eq)]
enum DefParam {
    Required(Option<String>),     // (opt) name; if None it's anon (rare for required)
    Optional(String),
    RestArg(Option<String>),      // *args or anon *
    KeywordReq(String),
    KeywordOpt(String),
    KwRestArg(Option<String>),    // **kw or anon **
    BlockArg(Option<String>),     // &blk or anon &
    Forward,                      // ...
}

fn collect_def_params<'a>(params: &ParametersNode<'a>) -> Vec<DefParam> {
    let mut out = Vec::new();
    for n in params.requireds().iter() {
        match n {
            Node::RequiredParameterNode { .. } => {
                let r = n.as_required_parameter_node().unwrap();
                let name = ConstId::name_string(r.name());
                out.push(DefParam::Required(Some(name)));
            }
            Node::MultiTargetNode { .. } => {
                // destructured arg — treat as required, no name (will not match)
                out.push(DefParam::Required(None));
            }
            _ => {}
        }
    }
    for n in params.optionals().iter() {
        if let Node::OptionalParameterNode { .. } = n {
            let r = n.as_optional_parameter_node().unwrap();
            out.push(DefParam::Optional(ConstId::name_string(r.name())));
        }
    }
    if let Some(rest) = params.rest() {
        match rest {
            Node::RestParameterNode { .. } => {
                let r = rest.as_rest_parameter_node().unwrap();
                let name = r
                    .name_loc()
                    .map(|l| String::from_utf8_lossy(l.as_slice()).into_owned());
                out.push(DefParam::RestArg(name));
            }
            Node::ImplicitRestNode { .. } => {
                out.push(DefParam::RestArg(None));
            }
            _ => {}
        }
    }
    for n in params.posts().iter() {
        if let Node::RequiredParameterNode { .. } = n {
            let r = n.as_required_parameter_node().unwrap();
            out.push(DefParam::Required(Some(ConstId::name_string(r.name()))));
        }
    }
    for n in params.keywords().iter() {
        match n {
            Node::RequiredKeywordParameterNode { .. } => {
                let r = n.as_required_keyword_parameter_node().unwrap();
                out.push(DefParam::KeywordReq(ConstId::name_string(r.name())));
            }
            Node::OptionalKeywordParameterNode { .. } => {
                let r = n.as_optional_keyword_parameter_node().unwrap();
                out.push(DefParam::KeywordOpt(ConstId::name_string(r.name())));
            }
            _ => {}
        }
    }
    if let Some(kw) = params.keyword_rest() {
        match kw {
            Node::KeywordRestParameterNode { .. } => {
                let r = kw.as_keyword_rest_parameter_node().unwrap();
                let name = r
                    .name_loc()
                    .map(|l| String::from_utf8_lossy(l.as_slice()).into_owned());
                out.push(DefParam::KwRestArg(name));
            }
            Node::ForwardingParameterNode { .. } => {
                out.push(DefParam::Forward);
            }
            Node::NoKeywordsParameterNode { .. } => {
                // **nil — treat as kwrest with no possible match
            }
            _ => {}
        }
    }
    if let Some(b) = params.block() {
        // anonymous block (`&` only) has no name_loc; named (`&blk`) has both name_loc and name.
        let is_anon = b.name_loc().is_none();
        let name = if is_anon {
            None
        } else {
            ConstId::name_string_opt(b.name())
        };
        out.push(DefParam::BlockArg(name));
    }
    out
}

// Helper: extract name string from a ConstantId
struct ConstId;
impl ConstId {
    fn name_string(c: ruby_prism::ConstantId<'_>) -> String {
        String::from_utf8_lossy(c.as_slice()).into_owned()
    }
    fn name_string_opt(c: Option<ruby_prism::ConstantId<'_>>) -> Option<String> {
        c.map(|x| String::from_utf8_lossy(x.as_slice()).into_owned())
    }
}

// ── Super-arg descriptors after preprocess (flatten unbraced hash) ──

#[derive(Debug)]
enum SuperArg {
    Lvar(String),                                  // local variable read
    Splat(Option<String>),                         // *x  (None = forwarded *)
    BlockPass(Option<String>),                     // &blk (None = forwarded &)
    AssocPair(String, String),                     // key sym, value lvar name
    AssocSplat(Option<String>),                    // **x in flattened hash
    AssocShorthand(String),                        // a:  (Ruby 3.1, value omitted == lvar a)
    ForwardedArgs,                                 // `...`
    Other,
}

fn preprocess_super_args(super_node: &SuperNode<'_>, source: &str) -> Option<Vec<SuperArg>> {
    let mut out: Vec<SuperArg> = Vec::new();
    // Prism stores BlockArgumentNode (e.g. `&blk`) in `super.block()`, not in `arguments()`.
    // Inline literal block `super(...) { ... }` also goes in `super.block()` (as BlockNode).
    // Treat BlockArgumentNode as a regular forwarded block arg — push to super_args.
    // BlockNode literal stays out of super_args (handled by inline-block trim).
    if let Some(args_node) = super_node.arguments() {
    for a in args_node.arguments().iter() {
        match &a {
            Node::HashNode { .. } => {
                let h = a.as_hash_node().unwrap();
                // Hash node is "braced" if its opening_loc spans `{`. In Prism 1.9, opening_loc
                // returns Location always; for unbraced (kwargs) it's an empty range.
                let ol = h.opening_loc();
                let has_braces = ol.end_offset() > ol.start_offset();
                if !has_braces {
                    // implicit/keyword hash — flatten its elements
                    for el in h.elements().iter() {
                        match el {
                            Node::AssocNode { .. } => {
                                push_assoc(&el, source, &mut out);
                            }
                            Node::AssocSplatNode { .. } => {
                                let sp = el.as_assoc_splat_node().unwrap();
                                let nm = sp
                                    .value()
                                    .as_ref()
                                    .and_then(|v| {
                                        if let Node::LocalVariableReadNode { .. } = v {
                                            let r = v.as_local_variable_read_node().unwrap();
                                            Some(ConstId::name_string(r.name()))
                                        } else {
                                            None
                                        }
                                    });
                                out.push(SuperArg::AssocSplat(nm));
                            }
                            _ => out.push(SuperArg::Other),
                        }
                    }
                } else {
                    out.push(SuperArg::Other);
                }
            }
            Node::KeywordHashNode { .. } => {
                let h = a.as_keyword_hash_node().unwrap();
                for el in h.elements().iter() {
                    match el {
                        Node::AssocNode { .. } => push_assoc(&el, source, &mut out),
                        Node::AssocSplatNode { .. } => {
                            let sp = el.as_assoc_splat_node().unwrap();
                            let nm = sp.value().as_ref().and_then(|v| {
                                if let Node::LocalVariableReadNode { .. } = v {
                                    let r = v.as_local_variable_read_node().unwrap();
                                    Some(ConstId::name_string(r.name()))
                                } else {
                                    None
                                }
                            });
                            out.push(SuperArg::AssocSplat(nm));
                        }
                        _ => out.push(SuperArg::Other),
                    }
                }
            }
            Node::LocalVariableReadNode { .. } => {
                let r = a.as_local_variable_read_node().unwrap();
                out.push(SuperArg::Lvar(ConstId::name_string(r.name())));
            }
            Node::SplatNode { .. } => {
                let s = a.as_splat_node().unwrap();
                let nm = s.expression().and_then(|e| {
                    if let Node::LocalVariableReadNode { .. } = e {
                        let r = e.as_local_variable_read_node().unwrap();
                        Some(ConstId::name_string(r.name()))
                    } else {
                        None
                    }
                });
                out.push(SuperArg::Splat(nm));
            }
            Node::BlockArgumentNode { .. } => {
                let b = a.as_block_argument_node().unwrap();
                let nm = b.expression().and_then(|e| {
                    if let Node::LocalVariableReadNode { .. } = e {
                        let r = e.as_local_variable_read_node().unwrap();
                        Some(ConstId::name_string(r.name()))
                    } else {
                        None
                    }
                });
                out.push(SuperArg::BlockPass(nm));
            }
            Node::ForwardingArgumentsNode { .. } => {
                out.push(SuperArg::ForwardedArgs);
            }
            _ => out.push(SuperArg::Other),
        }
    }
    }
    // Block arg in super.block() field
    if let Some(b) = super_node.block() {
        if let Node::BlockArgumentNode { .. } = &b {
            let ba = b.as_block_argument_node().unwrap();
            let nm = ba.expression().and_then(|e| {
                if let Node::LocalVariableReadNode { .. } = e {
                    let r = e.as_local_variable_read_node().unwrap();
                    Some(ConstId::name_string(r.name()))
                } else {
                    None
                }
            });
            out.push(SuperArg::BlockPass(nm));
        }
        // BlockNode (literal { ... } or do..end) is intentionally NOT pushed —
        // it's handled by the inline-block trim in arguments_identical.
    }
    Some(out)
}

/// Returns true if super has an inline literal block `{...}` or `do...end`.
fn super_has_literal_block(super_node: &SuperNode<'_>) -> bool {
    match super_node.block() {
        Some(b) => matches!(&b, Node::BlockNode { .. }),
        None => false,
    }
}

fn push_assoc(el: &Node<'_>, source: &str, out: &mut Vec<SuperArg>) {
    let assoc = el.as_assoc_node().unwrap();
    let key = assoc.key();
    let value = assoc.value();
    // Key must be symbol literal
    let key_name = match &key {
        Node::SymbolNode { .. } => {
            let s = key.as_symbol_node().unwrap();
            s.value_loc().map(|l| String::from_utf8_lossy(l.as_slice()).into_owned())
        }
        _ => None,
    };
    let Some(kname) = key_name else { out.push(SuperArg::Other); return; };

    // Hash value omission (Ruby 3.1+): `a:` — value will be a LocalVariableReadNode
    // whose location is INSIDE the key span (no explicit value source).
    // Detect by checking if value's location is contained within key's location.
    let key_loc = key.location();
    let val_loc = value.location();
    if val_loc.start_offset() >= key_loc.start_offset()
        && val_loc.end_offset() <= key_loc.end_offset()
    {
        // shorthand
        out.push(SuperArg::AssocShorthand(kname));
        return;
    }

    // Otherwise: value should be a LocalVariableRead with same name as key
    if let Node::LocalVariableReadNode { .. } = &value {
        let r = value.as_local_variable_read_node().unwrap();
        let vname = ConstId::name_string(r.name());
        // Compare key source to value source: in `b: b` both written `b` — if source slices match the key (without colon) and the var, it's identical
        // RuboCop checks `sym_node.source == lvar_node.source`. Sym source = `b:`, lvar source = `b`.
        // So we just need value's name == key's name (sym value).
        let _ = source;
        out.push(SuperArg::AssocPair(kname, vname));
        return;
    }
    out.push(SuperArg::Other);
}

// ── Match def params vs super args ──

fn arguments_identical(
    def_params: &[DefParam],
    super_args: &[SuperArg],
    block_reassigned_names: &[String],
    super_has_inline_block: bool,
) -> bool {
    // Block-arg adjustment: if def has BlockArg AND super has its own block (BlockNode literal),
    // ignore the def block arg from comparison.
    let def_iter: Vec<&DefParam> = if super_has_inline_block {
        def_params
            .iter()
            .filter(|p| !matches!(p, DefParam::BlockArg(_)))
            .collect()
    } else {
        def_params.iter().collect()
    };

    if def_iter.len() != super_args.len() {
        return false;
    }

    for (dp, sa) in def_iter.iter().zip(super_args.iter()) {
        if !param_matches(dp, sa, block_reassigned_names) {
            return false;
        }
    }
    true
}

fn param_matches(dp: &DefParam, sa: &SuperArg, block_reassigned: &[String]) -> bool {
    match (dp, sa) {
        // Required / Optional positional → lvar with same name
        (DefParam::Required(Some(n)), SuperArg::Lvar(v)) => n == v,
        (DefParam::Optional(n), SuperArg::Lvar(v)) => n == v,
        // Rest: anonymous *  → ForwardedRestArg (ruby 3.2 anon forwarding) or Splat(None)
        (DefParam::RestArg(None), SuperArg::Splat(None)) => true,
        (DefParam::RestArg(Some(n)), SuperArg::Splat(Some(v))) => n == v,
        // KeywordReq / KeywordOpt → AssocPair(sym=name, val=name) OR AssocShorthand(name)
        (DefParam::KeywordReq(n), SuperArg::AssocPair(k, v)) => n == k && k == v,
        (DefParam::KeywordReq(n), SuperArg::AssocShorthand(k)) => n == k,
        (DefParam::KeywordOpt(n), SuperArg::AssocPair(k, v)) => n == k && k == v,
        (DefParam::KeywordOpt(n), SuperArg::AssocShorthand(k)) => n == k,
        // KwRest: anonymous **  → AssocSplat(None)
        (DefParam::KwRestArg(None), SuperArg::AssocSplat(None)) => true,
        (DefParam::KwRestArg(Some(n)), SuperArg::AssocSplat(Some(v))) => n == v,
        // Block: same name and not reassigned
        (DefParam::BlockArg(None), SuperArg::BlockPass(None)) => true,
        (DefParam::BlockArg(Some(n)), SuperArg::BlockPass(Some(v))) => {
            n == v && !block_reassigned.iter().any(|r| r == n)
        }
        // Forward: super(...) matches def(...)
        (DefParam::Forward, SuperArg::ForwardedArgs) => true,
        _ => false,
    }
}

// ── Detect block reassignment of a name within def body ──

fn collect_reassigned_block_names_node(def_node: &DefNode<'_>, block_names: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    if block_names.is_empty() {
        return found;
    }
    let body_node = def_node.body();
    if let Some(b) = body_node {
        let mut v = AsgnVisitor { targets: block_names, found: &mut found };
        v.visit(&b);
    }
    found
}

struct AsgnVisitor<'a> {
    targets: &'a [String],
    found: &'a mut Vec<String>,
}

impl<'a, 'pr> Visit<'pr> for AsgnVisitor<'a> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        let name = ConstId::name_string(node.name());
        if self.targets.contains(&name) && !self.found.contains(&name) {
            self.found.push(name);
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        let name = ConstId::name_string(node.name());
        if self.targets.contains(&name) && !self.found.contains(&name) {
            self.found.push(name);
        }
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
}

// ── Find enclosing def for a super, walking up; bail if a block intervenes
//    (unless the block's send is the super itself / chain containing it). ──

// ── Cop ──

impl Cop for SuperArguments {
    fn name(&self) -> &'static str { "Style/SuperArguments" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode<'_>, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = SuperVisitor {
            stack: Vec::new(),
            offenses: Vec::new(),
            ctx,
            cop: self,
            _phantom: std::marker::PhantomData,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct DefInfo {
    params: Vec<DefParam>,
    /// names of block parameters that get reassigned in body
    reassigned_block_names: Vec<String>,
}

enum Frame {
    Def(DefInfo),
    BlockOpaque,         // any block — blocks super-checking
    Sclass,              // class << self — changes scope
    ClassOrModule,       // class/module def — changes scope
}

struct SuperVisitor<'a, 'pr> {
    stack: Vec<Frame>,
    offenses: Vec<Offense>,
    ctx: &'a CheckContext<'a>,
    cop: &'a SuperArguments,
    _phantom: std::marker::PhantomData<&'pr ()>,
}

impl<'a, 'pr> SuperVisitor<'a, 'pr> {
    fn enclosing_def(&self) -> Option<&DefInfo> {
        for f in self.stack.iter().rev() {
            match f {
                Frame::Def(d) => return Some(d),
                Frame::BlockOpaque | Frame::Sclass | Frame::ClassOrModule => return None,
            }
        }
        None
    }
}

impl<'a, 'pr> Visit<'pr> for SuperVisitor<'a, 'pr> {
    fn visit_def_node(&mut self, node: &DefNode<'pr>) {
        let params: Vec<DefParam> = match node.parameters() {
            Some(p) => collect_def_params(&p),
            None => Vec::new(),
        };
        let block_names: Vec<String> = params
            .iter()
            .filter_map(|p| match p {
                DefParam::BlockArg(Some(n)) => Some(n.clone()),
                _ => None,
            })
            .collect();
        let reassigned = collect_reassigned_block_names_node(node, &block_names);
        self.stack.push(Frame::Def(DefInfo {
            params,
            reassigned_block_names: reassigned,
        }));
        ruby_prism::visit_def_node(self, node);
        self.stack.pop();
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        self.stack.push(Frame::ClassOrModule);
        ruby_prism::visit_class_node(self, node);
        self.stack.pop();
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        self.stack.push(Frame::ClassOrModule);
        ruby_prism::visit_module_node(self, node);
        self.stack.pop();
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'pr>) {
        self.stack.push(Frame::Sclass);
        ruby_prism::visit_singleton_class_node(self, node);
        self.stack.pop();
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        // A block is "transparent" (allows super to refer to outer def) only if
        // its send/call expression IS the super being processed, OR contains it as a chain.
        // Approximation: scan block's body for a top-level SuperNode that the block surrounds.
        // Simpler: blocks always opaque here; we attach via parent CallNode logic instead.
        //
        // RuboCop's logic: walking up ancestors of super_node, if it hits a block,
        // it checks if that block.send_node "each_node(:super)" includes the super.
        // For a block like `define_method(:foo) { super(a) }`, the block's send_node
        // is `define_method(:foo)` which does NOT contain super → opaque.
        // But `super(a) { x }` parses as super-with-block — super IS the receiver of block,
        // not a block-on-call. So all "real" BlockNodes (inside CallNodes like
        // `define_method(:foo) do ... end`) are opaque from super's perspective.
        self.stack.push(Frame::BlockOpaque);
        ruby_prism::visit_block_node(self, node);
        self.stack.pop();
    }

    fn visit_super_node(&mut self, node: &SuperNode<'pr>) {
        // Process this super using enclosing def info (if any).
        let def_snapshot: Option<(Vec<DefParam>, Vec<String>)> = self
            .enclosing_def()
            .map(|d| (d.params.clone(), d.reassigned_block_names.clone()));
        if let Some((params, reassigned)) = def_snapshot {
            self.process_super(node, &params, &reassigned);
        }
        ruby_prism::visit_super_node(self, node);
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        // bare super — never an offense (we want to KEEP this form).
        ruby_prism::visit_forwarding_super_node(self, node);
    }
}

impl<'a, 'pr> SuperVisitor<'a, 'pr> {
    fn process_super(
        &mut self,
        super_node: &SuperNode<'pr>,
        def_params: &[DefParam],
        reassigned: &[String],
    ) {
        // Get super args (preprocessed)
        let super_args = preprocess_super_args(super_node, self.ctx.source).unwrap_or_default();

        // Inline block on super: super(...) { ... } where block field is a literal BlockNode.
        // BlockArgumentNode in block field has already been treated as a regular super arg.
        let super_has_inline_block = super_has_literal_block(super_node);

        if !arguments_identical(def_params, &super_args, reassigned, super_has_inline_block) {
            return;
        }

        // Determine message: MSG when full def matches; MSG_INLINE_BLOCK when def's block param
        // was trimmed because super has its own literal block.
        let msg = if def_params.len() == super_args.len() {
            MSG
        } else {
            MSG_INLINE_BLOCK
        };

        // Range: super keyword start through the closing paren (or last arg) end.
        let kw = super_node.keyword_loc();
        let start = kw.start_offset();
        let end = if let Some(rp) = super_node.rparen_loc() {
            rp.end_offset()
        } else if let Some(args_node) = super_node.arguments() {
            args_node.location().end_offset()
        } else {
            kw.end_offset()
        };

        // Replacement: replace the [start..end] with `super`
        let offense = self
            .ctx
            .offense_with_range(self.cop.name(), msg, self.cop.severity(), start, end)
            .with_correction(Correction::replace(start, end, "super".to_string()));
        self.offenses.push(offense);
    }
}

crate::register_cop!("Style/SuperArguments", |_cfg| Some(Box::new(SuperArguments::new())));
