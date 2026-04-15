//! Style/TrivialAccessors - Looks for trivial reader/writer methods.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/trivial_accessors.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/TrivialAccessors";

pub struct TrivialAccessors {
    allowed_methods: Vec<String>,
    exact_name_match: bool,
    allow_predicates: bool,
    allow_dsl_writers: bool,
    ignore_class_methods: bool,
}

impl Default for TrivialAccessors {
    fn default() -> Self {
        Self {
            allowed_methods: vec![
                "to_ary", "to_a", "to_c", "to_enum", "to_h", "to_hash", "to_i", "to_int", "to_io",
                "to_open", "to_path", "to_proc", "to_r", "to_regexp", "to_str", "to_s", "to_sym",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            exact_name_match: true,
            allow_predicates: true,
            allow_dsl_writers: true,
            ignore_class_methods: false,
        }
    }
}

impl TrivialAccessors {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(
        allowed_methods: Vec<String>,
        exact_name_match: bool,
        allow_predicates: bool,
        allow_dsl_writers: bool,
        ignore_class_methods: bool,
    ) -> Self {
        Self { allowed_methods, exact_name_match, allow_predicates, allow_dsl_writers, ignore_class_methods }
    }
}

impl Cop for TrivialAccessors {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = TAVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            scope_stack: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Class,
    Sclass,
    Module,
    InstanceEval,
    OtherBlock,
}

struct TAVisitor<'a> {
    cop: &'a TrivialAccessors,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    scope_stack: Vec<Scope>,
}

impl<'a> TAVisitor<'a> {
    /// Mirror RuboCop `in_module_or_instance_eval?` — walk up ancestors.
    /// Return true if should SKIP. First class/sclass found → false (don't skip).
    /// Module or instance_eval → true (skip).
    fn in_module_or_instance_eval(&self) -> bool {
        for s in self.scope_stack.iter().rev() {
            match s {
                Scope::Class | Scope::Sclass => return false,
                Scope::Module => return true,
                Scope::InstanceEval => return true,
                Scope::OtherBlock => {} // keep walking
            }
        }
        false
    }

    /// Top level means no enclosing class/sclass/module.
    fn is_top_level(&self) -> bool {
        !self.scope_stack.iter().any(|s| matches!(s, Scope::Class | Scope::Sclass | Scope::Module))
    }

    fn is_predicate_name(name: &str) -> bool { name.ends_with('?') }
    fn is_assignment_name(name: &str) -> bool { name.ends_with('=') }

    fn names_match(&self, method_name: &str, ivar_name: &str) -> bool {
        // RuboCop: node.method_name.to_s.sub(/[=?]$/, '') == ivar_name[1..]
        let base = method_name.trim_end_matches(['=', '?']);
        let ivar_base = ivar_name.strip_prefix('@').unwrap_or(ivar_name);
        base == ivar_base
    }

    fn allowed_method_name(&self, method_name: &str) -> bool {
        let always_allowed = method_name == "initialize";
        if always_allowed { return true; }
        if self.cop.allowed_methods.iter().any(|m| m == method_name) {
            return true;
        }
        // exact_name_match? && !names_match? — handled at call site (need ivar).
        false
    }

    /// Check the DefNode body for a trivial reader/writer and return the single-ivar if found.
    fn body_reader_ivar<'b>(&self, body: &Option<Node<'b>>) -> Option<String> {
        let body = body.as_ref()?;
        // Unwrap StatementsNode with single child
        if let Some(stmts) = body.as_statements_node() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 { return None; }
            if let Some(iv) = items[0].as_instance_variable_read_node() {
                return Some(node_name!(iv).to_string());
            }
            return None;
        }
        // Non-StatementsNode body (shouldn't normally happen for def body)
        if let Some(iv) = body.as_instance_variable_read_node() {
            return Some(node_name!(iv).to_string());
        }
        None
    }

    /// Writer body: `@foo = <lvar>`
    fn body_writer_ivar<'b>(&self, body: &Option<Node<'b>>, param_name: &str) -> Option<String> {
        let body = body.as_ref()?;
        let (write_node, _) = if let Some(stmts) = body.as_statements_node() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 { return None; }
            let write = items[0].as_instance_variable_write_node()?;
            (write, ())
        } else {
            let write = body.as_instance_variable_write_node()?;
            (write, ())
        };
        let ivar_name = node_name!(write_node).to_string();
        let value = write_node.value();
        if let Some(lvr) = value.as_local_variable_read_node() {
            if node_name!(lvr) == param_name {
                return Some(ivar_name);
            }
        }
        None
    }

    /// Check a def/defs node. For defs, only flag if receiver is `self`.
    fn check_def(&mut self, node: &ruby_prism::DefNode) {
        // Determine if def_self (defs) vs plain def.
        let is_defs = node.receiver().is_some();

        if is_defs {
            // Only flag if receiver is self (RuboCop autocorrect constraint).
            let recv = node.receiver().unwrap();
            if recv.as_self_node().is_none() {
                return;
            }
            if self.cop.ignore_class_methods {
                return;
            }
        }

        if self.is_top_level() {
            return;
        }
        if self.in_module_or_instance_eval() {
            return;
        }

        let method_name_bytes = node.name();
        let method_name = String::from_utf8_lossy(method_name_bytes.as_slice()).to_string();

        // Check reader first: no parameters + body is single ivar read
        let params = node.parameters();
        let no_params = params.as_ref().map_or(true, |p| {
            let req = p.requireds().iter().count();
            let opt = p.optionals().iter().count();
            let post = p.posts().iter().count();
            let kwd = p.keywords().iter().count();
            let blk = p.block().is_some();
            let rest = p.rest().is_some();
            let kwd_rest = p.keyword_rest().is_some();
            req + opt + post + kwd == 0 && !blk && !rest && !kwd_rest
        });

        let body = node.body();

        // Allowed method name (always allowed like initialize, to_s, etc.)
        if self.allowed_method_name(&method_name) {
            return;
        }

        // Try reader path
        if no_params {
            if let Some(ivar) = self.body_reader_ivar(&body) {
                // Predicate: foo? - if AllowPredicates true, skip
                if Self::is_predicate_name(&method_name) && self.cop.allow_predicates {
                    return;
                }
                // ExactNameMatch: require method_name base == ivar name base
                if self.cop.exact_name_match && !self.names_match(&method_name, &ivar) {
                    return;
                }
                self.emit(node, "reader");
                return;
            }
        }

        // Writer path: exactly one required param, body `@x = <that param>`.
        let (single_required, param_name) = if let Some(p) = &params {
            let req: Vec<_> = p.requireds().iter().collect();
            let opt = p.optionals().iter().count();
            let post = p.posts().iter().count();
            let kwd = p.keywords().iter().count();
            let blk = p.block().is_some();
            let rest = p.rest().is_some();
            let kwd_rest = p.keyword_rest().is_some();
            if req.len() == 1 && opt == 0 && post == 0 && kwd == 0 && !blk && !rest && !kwd_rest {
                // param must be a simple required arg (RequiredParameterNode)
                if let Some(rp) = req[0].as_required_parameter_node() {
                    let pname = node_name!(rp).to_string();
                    (true, pname)
                } else {
                    (false, String::new())
                }
            } else {
                (false, String::new())
            }
        } else {
            (false, String::new())
        };

        if single_required {
            if let Some(ivar) = self.body_writer_ivar(&body, &param_name) {
                // DSL writer: if method name doesn't end with '='
                let is_assignment = Self::is_assignment_name(&method_name);
                if !is_assignment && self.cop.allow_dsl_writers {
                    return;
                }
                if self.cop.exact_name_match && !self.names_match(&method_name, &ivar) {
                    return;
                }
                self.emit(node, "writer");
            }
        }
    }

    fn emit(&mut self, node: &ruby_prism::DefNode, kind: &str) {
        let kw = node.def_keyword_loc();
        let msg = format!("Use `attr_{}` to define trivial {} methods.", kind, kind);
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &msg, Severity::Convention,
            kw.start_offset(), kw.end_offset(),
        ));
    }
}

impl<'a> Visit<'_> for TAVisitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.scope_stack.push(Scope::Class);
        ruby_prism::visit_class_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        self.scope_stack.push(Scope::Sclass);
        ruby_prism::visit_singleton_class_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.scope_stack.push(Scope::Module);
        ruby_prism::visit_module_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // If this call has a block and method is instance_eval, push InstanceEval.
        let method = node_name!(node);
        let pushed = if node.block().is_some() {
            let scope = if method == "instance_eval" { Scope::InstanceEval } else { Scope::OtherBlock };
            self.scope_stack.push(scope);
            true
        } else {
            false
        };
        ruby_prism::visit_call_node(self, node);
        if pushed {
            self.scope_stack.pop();
        }
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.check_def(node);
        ruby_prism::visit_def_node(self, node);
    }
}
