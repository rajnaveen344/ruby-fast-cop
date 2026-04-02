//! Types used by the variable force analysis.

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    Simple,
    MultiAssign,
    OpAssign,   // +=, -=, etc.
    AndAssign,  // &&=
    OrAssign,   // ||=
    RegexpCapture,
}

#[derive(Debug, Clone)]
pub struct WriteInfo {
    pub name: String,
    pub name_start: usize,
    pub name_end: usize,
    pub kind: WriteKind,
    pub op: Option<String>,
    pub regexp_start: usize,
    pub regexp_end: usize,
}

pub struct ScopeInfo {
    pub params: HashSet<String>,
    pub has_bare_super: bool,
    pub method_calls: HashSet<String>,
    pub all_var_names: HashSet<String>,
    pub all_reads: HashSet<String>,
}

impl ScopeInfo {
    pub fn new() -> Self {
        Self {
            params: HashSet::new(),
            has_bare_super: false,
            method_calls: HashSet::new(),
            all_var_names: HashSet::new(),
            all_reads: HashSet::new(),
        }
    }
}
