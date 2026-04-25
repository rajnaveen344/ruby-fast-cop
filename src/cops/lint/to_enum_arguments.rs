//! Lint/ToEnumArguments
//!
//! Ensures `to_enum`/`enum_for` called for the current method has correct args.
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/to_enum_arguments.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, ParametersNode, Visit};

#[derive(Default)]
pub struct ToEnumArguments;

impl ToEnumArguments {
    pub fn new() -> Self { Self }
}

impl Cop for ToEnumArguments {
    fn name(&self) -> &'static str { "Lint/ToEnumArguments" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = ToEnumVisitor { ctx, def_stack: Vec::new(), offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct ToEnumVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    def_stack: Vec<DefInfo<'a>>,
    offenses: Vec<Offense>,
}

struct DefInfo<'a> {
    method_name: String,
    params: Option<ParametersNode<'a>>,
}

impl<'a> Visit<'a> for ToEnumVisitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'a>) {
        let name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
        self.def_stack.push(DefInfo { method_name: name, params: node.parameters() });
        ruby_prism::visit_def_node(self, node);
        self.def_stack.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> ToEnumVisitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode<'a>) {
        let m_cow = String::from_utf8_lossy(node.name().as_slice());
        let m = m_cow.as_ref();
        if m != "to_enum" && m != "enum_for" {
            return;
        }
        // Receiver must be nil or self
        if let Some(recv) = node.receiver() {
            if !matches!(recv, Node::SelfNode { .. }) {
                return;
            }
        }
        let def_info = match self.def_stack.last() {
            Some(d) => d,
            None => return,
        };
        // Need first arg = method name matching enclosing def
        let args_opt = node.arguments();
        let args: Vec<Node<'a>> = match &args_opt {
            Some(a) => a.arguments().iter().collect(),
            None => return,
        };
        if args.is_empty() {
            return;
        }
        if !is_method_name(&args[0], &def_info.method_name) {
            return;
        }
        // Avoid Node Clone: re-iterate to skip first.
        let send_args: Vec<Node<'a>> = match &args_opt {
            Some(a) => a.arguments().iter().skip(1).collect(),
            None => Vec::new(),
        };
        if Self::arguments_match(&send_args, def_info.params.as_ref(), self.ctx.source) {
            return;
        }

        let loc = node.location();
        self.offenses.push(self.ctx.offense(
            "Lint/ToEnumArguments",
            "Ensure you correctly provided all the arguments.",
            Severity::Warning,
            &loc,
        ));
    }

    fn arguments_match(send_args: &[Node<'a>], params: Option<&ParametersNode<'a>>, src: &str) -> bool {
        let params = match params { Some(p) => p, None => return send_args.is_empty() };
        let mut idx: usize = 0;

        for req in params.requireds().iter() {
            if matches!(req, Node::ForwardingParameterNode { .. }) {
                continue; // handled below
            }
            let pname = match req.as_required_parameter_node() {
                Some(p) => String::from_utf8_lossy(p.name().as_slice()).into_owned(),
                None => return false,
            };
            let send_arg = match send_args.get(idx) { Some(a) => a, None => return false };
            idx += 1;
            if node_src(send_arg, src) != pname {
                return false;
            }
        }

        for opt in params.optionals().iter() {
            let pname = match opt.as_optional_parameter_node() {
                Some(p) => String::from_utf8_lossy(p.name().as_slice()).into_owned(),
                None => return false,
            };
            let send_arg = match send_args.get(idx) { Some(a) => a, None => return false };
            idx += 1;
            if node_src(send_arg, src) != pname {
                return false;
            }
        }

        if let Some(rest) = params.rest() {
            // RestParameterNode source like `*args` or `*`
            let rest_src = node_src(&rest, src);
            let send_arg = match send_args.get(idx) { Some(a) => a, None => return false };
            idx += 1;
            if node_src(send_arg, src) != rest_src {
                return false;
            }
        }

        // keywords (kwarg / kwoptarg)
        let keywords: Vec<Node<'a>> = params.keywords().iter().collect();
        if !keywords.is_empty() {
            // Need a hash arg holding pair (sym name) (lvar name) for each kwarg
            // Forwarding (...) also acceptable: if forwarded args present, accept.
            let send_arg = send_args.get(idx);
            for kw in &keywords {
                let kw_name = if let Some(p) = kw.as_required_keyword_parameter_node() {
                    String::from_utf8_lossy(p.name().as_slice()).into_owned()
                } else if let Some(p) = kw.as_optional_keyword_parameter_node() {
                    String::from_utf8_lossy(p.name().as_slice()).into_owned()
                } else {
                    return false;
                };
                let arg = match send_arg { Some(a) => a, None => return false };
                if !hash_has_passing_kwarg(arg, &kw_name) {
                    return false;
                }
            }
        }

        if let Some(kwrest) = params.keyword_rest() {
            if !matches!(kwrest, Node::ForwardingParameterNode { .. }) {
                let kwrest_src = node_src(&kwrest, src);
                let send_arg = match send_args.last() { Some(a) => a, None => return false };
                if !hash_has_kwsplat(send_arg, &kwrest_src, src) {
                    return false;
                }
            }
        }

        // forward_arg: ParametersNode has no `forwarding()` accessor — detect via source
        // Check for `...` parameter — we approximate by scanning params source.
        // Simpler: check_def gives ForwardingParameterNode via .keywords()? Actually it's stored
        // separately. Use a heuristic: check for ForwardingArgumentsNode in send_args matching `...` param.
        // RuboCop check: forward_arg type → send_arg.forwarded_args_type?
        // We detect ForwardingParameter on params via iterating all children.
        if has_forwarding_parameter(params) {
            // Send_args must contain a forwarded args node at this position
            let send_arg = send_args.get(idx);
            match send_arg {
                Some(Node::ForwardingArgumentsNode { .. }) => {}
                _ => return false,
            }
        }

        true
    }
}

fn node_src<'a>(node: &Node<'a>, src: &str) -> String {
    let loc = node.location();
    src[loc.start_offset()..loc.end_offset()].to_string()
}

fn is_method_name(node: &Node, target: &str) -> bool {
    // (sym :name) or (send nil? {:__method__ :__callee__})
    if let Some(sym) = node.as_symbol_node() {
        let bytes = sym.unescaped();
        return std::str::from_utf8(bytes.as_ref()).map(|s| s == target).unwrap_or(false);
    }
    if let Some(call) = node.as_call_node() {
        if call.receiver().is_some() { return false; }
        if call.arguments().map_or(false, |a| a.arguments().iter().count() > 0) { return false; }
        let m = String::from_utf8_lossy(call.name().as_slice()).into_owned();
        return m == "__method__" || m == "__callee__";
    }
    false
}

fn hash_has_passing_kwarg(arg: &Node, name: &str) -> bool {
    let mut pairs: Vec<Node> = Vec::new();
    if let Some(k) = arg.as_keyword_hash_node() {
        for p in k.elements().iter() { pairs.push(p); }
    } else if let Some(h) = arg.as_hash_node() {
        for p in h.elements().iter() { pairs.push(p); }
    } else {
        return false;
    }
    for p in &pairs {
        if let Some(assoc) = p.as_assoc_node() {
            // key = sym(name), value = lvar(name)
            let key = assoc.key();
            let val = assoc.value();
            let key_name = sym_name(&key);
            let val_name = lvar_name(&val);
            if key_name.as_deref() == Some(name) && val_name.as_deref() == Some(name) {
                return true;
            }
        }
    }
    false
}

fn hash_has_kwsplat(arg: &Node, kwrest_src: &str, src: &str) -> bool {
    let mut pairs: Vec<Node> = Vec::new();
    if let Some(k) = arg.as_keyword_hash_node() {
        for p in k.elements().iter() { pairs.push(p); }
    } else if let Some(h) = arg.as_hash_node() {
        for p in h.elements().iter() { pairs.push(p); }
    } else {
        return false;
    }
    for p in &pairs {
        if matches!(p, Node::AssocSplatNode { .. }) {
            let s = node_src(p, src);
            if s == kwrest_src { return true; }
        }
    }
    false
}

fn sym_name(node: &Node) -> Option<String> {
    let sym = node.as_symbol_node()?;
    let v = sym.unescaped();
    Some(String::from_utf8_lossy(v.as_ref()).into_owned())
}

fn lvar_name(node: &Node) -> Option<String> {
    let lv = node.as_local_variable_read_node()?;
    Some(String::from_utf8_lossy(lv.name().as_slice()).into_owned())
}

fn has_forwarding_parameter(params: &ParametersNode) -> bool {
    for p in params.requireds().iter() {
        if matches!(p, Node::ForwardingParameterNode { .. }) { return true; }
    }
    if let Some(kr) = params.keyword_rest() {
        if matches!(kr, Node::ForwardingParameterNode { .. }) { return true; }
    }
    false
}

crate::register_cop!("Lint/ToEnumArguments", |_cfg| Some(Box::new(ToEnumArguments::new())));
