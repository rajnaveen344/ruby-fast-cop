//! Naming/PredicatePrefix cop
//!
//! Checks that method names starting with `is_` or `has_` are renamed to
//! use the `?` suffix instead.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/naming/predicate_prefix.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct PredicatePrefix {
    forbidden_prefixes: Vec<String>,
    name_prefix: Vec<String>,
    allowed_methods: Vec<String>,
    method_definition_macros: Vec<String>,
    use_sorbet_sigs: bool,
}

impl Default for PredicatePrefix {
    fn default() -> Self {
        Self {
            forbidden_prefixes: vec!["is_".to_string(), "has_".to_string()],
            name_prefix: vec!["is_".to_string(), "has_".to_string()],
            allowed_methods: vec!["is_a?".to_string()],
            method_definition_macros: vec![
                "define_method".to_string(),
                "define_singleton_method".to_string(),
            ],
            use_sorbet_sigs: false,
        }
    }
}

impl PredicatePrefix {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        forbidden_prefixes: Vec<String>,
        name_prefix: Vec<String>,
        allowed_methods: Vec<String>,
        method_definition_macros: Vec<String>,
        use_sorbet_sigs: bool,
    ) -> Self {
        Self {
            forbidden_prefixes,
            name_prefix,
            allowed_methods,
            method_definition_macros,
            use_sorbet_sigs,
        }
    }

    /// Find a matching name prefix for this method name.
    fn matching_prefix<'a>(&'a self, name: &str) -> Option<&'a str> {
        self.name_prefix.iter().find_map(|prefix| {
            if name.starts_with(prefix.as_str()) {
                Some(prefix.as_str())
            } else {
                None
            }
        })
    }

    /// Compute the corrected name.
    /// If ForbiddenPrefixes is non-empty AND contains the prefix → strip prefix, add ?
    /// Otherwise → just add ? (keep prefix)
    fn corrected_name(&self, name: &str, prefix: &str) -> String {
        if !self.forbidden_prefixes.is_empty() && self.forbidden_prefixes.contains(&prefix.to_string()) {
            // Strip prefix, add ?
            let base = &name[prefix.len()..];
            format!("{}?", base)
        } else {
            // Just add ?
            format!("{}?", name)
        }
    }

    /// Check if the corrected name would be a valid identifier (must start with alpha/underscore,
    /// no leading digit).
    fn corrected_name_is_valid(&self, name: &str, prefix: &str) -> bool {
        let corrected = self.corrected_name(name, prefix);
        // Strip the trailing `?` for identifier check
        let base = corrected.trim_end_matches('?');
        if base.is_empty() {
            return false;
        }
        let first = base.as_bytes()[0];
        first.is_ascii_alphabetic() || first == b'_'
    }

    fn is_allowed(&self, name: &str) -> bool {
        self.allowed_methods.contains(&name.to_string())
    }

    fn check_method_name(
        &self,
        name: &str,
        start: usize,
        end: usize,
        ctx: &CheckContext,
        cop_name: &'static str,
        preceding_sig: Option<bool>, // Some(true) = returns T::Boolean, Some(false) = returns other
    ) -> Vec<Offense> {
        // Skip assignment methods
        if name.ends_with('=') {
            return vec![];
        }

        // Skip methods already ending with ?
        if name.ends_with('?') {
            return vec![];
        }

        // If use_sorbet_sigs: only flag if preceding sig returns T::Boolean
        if self.use_sorbet_sigs {
            match preceding_sig {
                None => return vec![], // No sig, skip
                Some(false) => return vec![], // Non-boolean sig, skip
                Some(true) => {} // Boolean sig, continue
            }
        }

        if self.is_allowed(name) {
            return vec![];
        }

        let prefix = match self.matching_prefix(name) {
            Some(p) => p,
            None => return vec![],
        };

        if !self.corrected_name_is_valid(name, prefix) {
            return vec![];
        }

        let corrected = self.corrected_name(name, prefix);
        let msg = format!("Rename `{}` to `{}`.", name, corrected);

        vec![ctx.offense_with_range(cop_name, &msg, Severity::Convention, start, end)]
    }
}

/// Visitor for PredicatePrefix
struct PredicatePrefixVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a PredicatePrefix,
    offenses: Vec<Offense>,
    /// Preceding sorbet sig: tracks whether the last seen `sig { ... }` / `sig do...end`
    /// returns T::Boolean.
    preceding_sig_returns_boolean: Option<bool>,
    /// Line number where the preceding sig ends (so we can decide if it applies to next def)
    preceding_sig_end_line: Option<usize>,
}

impl<'a> PredicatePrefixVisitor<'a> {
    fn is_macro(&self, name: &str) -> bool {
        self.cop.method_definition_macros.contains(&name.to_string())
    }

    fn applicable_sig(&self, def_line: usize) -> Option<bool> {
        if !self.cop.use_sorbet_sigs {
            return None;
        }
        // The sig applies if it was on a preceding line (allowing blanks/comments in between)
        // We just return the last sig's boolean status if it exists
        self.preceding_sig_returns_boolean
    }

    /// Check if a call is a sorbet sig block: `sig { returns(T::Boolean) }` or `sig do...end`
    fn extract_sig_returns_boolean(node: &ruby_prism::CallNode) -> Option<bool> {
        let name = node_name!(node);
        if name != "sig" {
            return None;
        }
        // Has a block
        let block = node.block()?;
        let block_node = block.as_block_node()?;
        let body = block_node.body()?;
        let stmts = body.as_statements_node()?;
        let items: Vec<_> = stmts.body().iter().collect();
        for item in &items {
            if Self::node_returns_boolean(item) {
                return Some(true);
            }
        }
        Some(false)
    }

    /// Check if a node is `returns(T::Boolean)` or has it in a chain
    fn node_returns_boolean(node: &Node) -> bool {
        let call = match node.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let method = node_name!(call);
        if method == "returns" {
            if let Some(args) = call.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if arg_list.len() == 1 && Self::is_t_boolean(&arg_list[0]) {
                    return true;
                }
            }
        }
        // Check receiver chain: `params(...).returns(T::Boolean)`
        if let Some(recv) = call.receiver() {
            if Self::node_returns_boolean(&recv) {
                return true;
            }
        }
        false
    }

    /// Check if a node is `T::Boolean`
    fn is_t_boolean(node: &Node) -> bool {
        let cp = match node.as_constant_path_node() {
            Some(c) => c,
            None => return false,
        };
        // parent should be T, name should be Boolean
        let parent = match cp.parent() {
            Some(p) => p,
            None => return false,
        };
        let parent_const = match parent.as_constant_read_node() {
            Some(c) => c,
            None => return false,
        };
        let parent_name = String::from_utf8_lossy(parent_const.name().as_slice());
        let name = cp.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string());
        parent_name == "T" && name.as_deref() == Some("Boolean")
    }
}

impl Visit<'_> for PredicatePrefixVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let name = node_name!(node).to_string();
        let name_loc = node.name_loc();

        let sig = self.applicable_sig(self.ctx.line_of(name_loc.start_offset()));

        let offenses = self.cop.check_method_name(
            &name,
            name_loc.start_offset(),
            name_loc.end_offset(),
            self.ctx,
            "Naming/PredicatePrefix",
            sig,
        );
        self.offenses.extend(offenses);

        // Reset sig after consuming it
        self.preceding_sig_returns_boolean = None;

        ruby_prism::visit_def_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node).to_string();

        // Check for sorbet sig
        if self.cop.use_sorbet_sigs && method == "sig" {
            if let Some(is_bool) = PredicatePrefixVisitor::extract_sig_returns_boolean(node) {
                self.preceding_sig_returns_boolean = Some(is_bool);
                self.preceding_sig_end_line = Some(self.ctx.line_of(node.location().end_offset()));
            }
            ruby_prism::visit_call_node(self, node);
            return;
        }

        // Check for method definition macros: define_method(:is_hello) do ... end
        if self.is_macro(&method) {
            // Check first argument as method name
            if let Some(args) = node.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if !arg_list.is_empty() {
                    let name_str = match &arg_list[0] {
                        Node::SymbolNode { .. } => {
                            let sym = arg_list[0].as_symbol_node().unwrap();
                            Some(String::from_utf8_lossy(sym.unescaped().as_ref()).to_string())
                        }
                        Node::StringNode { .. } => {
                            let s = arg_list[0].as_string_node().unwrap();
                            Some(String::from_utf8_lossy(s.unescaped().as_ref()).to_string())
                        }
                        _ => None,
                    };

                    if let Some(name) = name_str {
                        let loc = arg_list[0].location();
                        // RuboCop reports the full symbol/string location (including : or quotes)
                        let (start, end) = (loc.start_offset(), loc.end_offset());

                        let sig = self.applicable_sig(self.ctx.line_of(start));
                        let offenses = self.cop.check_method_name(
                            &name,
                            start,
                            end,
                            self.ctx,
                            "Naming/PredicatePrefix",
                            sig,
                        );
                        self.offenses.extend(offenses);
                        self.preceding_sig_returns_boolean = None;
                    }
                }
            }
        } else {
            // Non-def, non-macro call: reset sig tracking
            // Only reset if we're not inside the sig itself
        }

        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for PredicatePrefix {
    fn name(&self) -> &'static str {
        "Naming/PredicatePrefix"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = PredicatePrefixVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            preceding_sig_returns_boolean: None,
            preceding_sig_end_line: None,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    forbidden_prefixes: Option<Vec<String>>,
    name_prefix: Option<Vec<String>>,
    allowed_methods: Option<Vec<String>>,
    method_definition_macros: Option<Vec<String>>,
    use_sorbet_sigs: serde_yaml::Value,
}

crate::register_cop!("Naming/PredicatePrefix", |cfg| {
    let c: Cfg = cfg.typed("Naming/PredicatePrefix");
    // Use configured value if present (even empty), else use default
    let forbidden_prefixes = c.forbidden_prefixes
        .unwrap_or_else(|| vec!["is_".to_string(), "has_".to_string()]);
    let name_prefix = c.name_prefix
        .unwrap_or_else(|| vec!["is_".to_string(), "has_".to_string()]);
    let allowed_methods = c.allowed_methods
        .unwrap_or_else(|| vec!["is_a?".to_string()]);
    let method_definition_macros = c.method_definition_macros
        .unwrap_or_else(|| vec!["define_method".to_string(), "define_singleton_method".to_string()]);
    // UseSorbetSigs can be bool or string "true"/"false"
    let use_sorbet_sigs = match &c.use_sorbet_sigs {
        serde_yaml::Value::Bool(b) => *b,
        serde_yaml::Value::String(s) => s == "true",
        _ => false,
    };
    Some(Box::new(PredicatePrefix::with_config(
        forbidden_prefixes,
        name_prefix,
        allowed_methods,
        method_definition_macros,
        use_sorbet_sigs,
    )))
});
