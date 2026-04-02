//! Scope: a context of local variable visibility (mirrors RuboCop's Scope).

use super::variable::Variable;
use std::collections::HashMap;

/// A scope represents a context where local variables live.
/// Corresponds to def, class, module, block, or top-level.
pub struct Scope {
    /// Byte offset of the scope node (used as identity)
    pub node_offset: usize,
    /// End offset of the scope node
    pub node_end_offset: usize,
    /// What kind of scope this is
    pub scope_type: ScopeType,
    /// Variables declared in this scope
    pub variables: HashMap<String, Variable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeType {
    TopLevel,
    Def,
    Defs,
    Class,
    Module,
    SingletonClass,
    Block,
    Lambda,
}

impl Scope {
    pub fn new(node_offset: usize, node_end_offset: usize, scope_type: ScopeType) -> Self {
        Self {
            node_offset,
            node_end_offset,
            scope_type,
            variables: HashMap::new(),
        }
    }

    pub fn is_def(&self) -> bool {
        matches!(self.scope_type, ScopeType::Def | ScopeType::Defs)
    }

    pub fn is_block(&self) -> bool {
        matches!(self.scope_type, ScopeType::Block | ScopeType::Lambda)
    }
}
