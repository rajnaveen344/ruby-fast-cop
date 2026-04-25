//! Lint/DuplicateBranch cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_branch.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct DuplicateBranch {
    ignore_literal_branches: bool,
    ignore_constant_branches: bool,
    ignore_duplicate_else_branch: bool,
}

impl Default for DuplicateBranch {
    fn default() -> Self {
        Self {
            ignore_literal_branches: false,
            ignore_constant_branches: false,
            ignore_duplicate_else_branch: false,
        }
    }
}

impl DuplicateBranch {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(literal: bool, constant: bool, dup_else: bool) -> Self {
        Self {
            ignore_literal_branches: literal,
            ignore_constant_branches: constant,
            ignore_duplicate_else_branch: dup_else,
        }
    }
}

impl Cop for DuplicateBranch {
    fn name(&self) -> &'static str { "Lint/DuplicateBranch" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = BranchVisitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct BranchVisitor<'a> {
    cop: &'a DuplicateBranch,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

#[derive(Debug, Clone, Copy)]
enum BranchKind {
    /// then-branch of an if/unless or elsif. Range = parent IfNode/UnlessNode source_range.
    IfBranch { parent_start: usize, parent_end: usize, is_else: bool, is_ternary: bool },
    /// when branch. Range = parent WhenNode source_range.
    WhenBranch { start: usize, end: usize },
    /// rescue resbody. Range = parent RescueNode source_range.
    RescueBranch { start: usize, end: usize },
    /// else branch. Range = `else` keyword location.
    ElseBranch { start: usize, end: usize },
    /// ternary then/else. Range = body itself.
    TernaryBody { start: usize, end: usize },
}

struct Branch<'src> {
    /// Body source string (canonicalized = raw text). Empty branches → None body.
    body_src: Option<String>,
    /// Body Node (for is_literal/is_const checks).
    body_node: Option<Node<'src>>,
    /// Offense range
    kind: BranchKind,
}

impl<'src> Branch<'src> {
    fn offense_range(&self) -> (usize, usize) {
        match self.kind {
            BranchKind::IfBranch { parent_start, parent_end, .. } => (parent_start, parent_end),
            BranchKind::WhenBranch { start, end } => (start, end),
            BranchKind::RescueBranch { start, end } => (start, end),
            BranchKind::ElseBranch { start, end } => (start, end),
            BranchKind::TernaryBody { start, end } => (start, end),
        }
    }
}

/// Returns true if `node` is "elsif" (its keyword text is `elsif`).
fn is_elsif(node: &ruby_prism::IfNode, source: &str) -> bool {
    if let Some(kw) = node.if_keyword_loc() {
        let kw_src = &source[kw.start_offset()..kw.end_offset()];
        kw_src == "elsif"
    } else {
        false
    }
}

/// Returns true if `node` is a ternary (`a ? b : c`).
/// In Prism, ternary IfNodes have `if_keyword_loc == None` OR the keyword text is `?`.
fn is_ternary(node: &ruby_prism::IfNode, source: &str) -> bool {
    match node.if_keyword_loc() {
        Some(kw) => {
            let kw_src = &source[kw.start_offset()..kw.end_offset()];
            kw_src == "?"
        }
        None => true,
    }
}

/// Returns true if `node` is a modifier-form if/unless (`expr if cond`).
fn is_modifier(node_loc_start: usize, body_start: Option<usize>) -> bool {
    match body_start {
        Some(b) => b < node_loc_start,
        None => false,
    }
}

fn body_text<'src>(stmts: &Option<ruby_prism::StatementsNode<'src>>, source: &str) -> Option<String> {
    let s = stmts.as_ref()?;
    let items: Vec<Node> = s.body().iter().collect();
    if items.is_empty() { return None; }
    let start = items.first().unwrap().location().start_offset();
    let end = items.last().unwrap().location().end_offset();
    Some(source[start..end].to_string())
}

fn body_single_node<'src>(stmts: &Option<ruby_prism::StatementsNode<'src>>) -> Option<Node<'src>> {
    let s = stmts.as_ref()?;
    let items: Vec<Node> = s.body().iter().collect();
    if items.len() == 1 {
        return Some(items.into_iter().next().unwrap());
    }
    None
}

fn node_to_text(n: &Node, source: &str) -> String {
    let loc = n.location();
    source[loc.start_offset()..loc.end_offset()].to_string()
}

/// Check if a body Node is "literal" per RuboCop definition (recursively basic_literal).
/// Returns true also for arrays/hashes/regexps/ranges containing only literals.
/// Excludes xstr (`...`).
fn is_literal_body(n: &Node, ignore_constants: bool) -> bool {
    if matches!(n,
        Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. }
    ) {
        return false;
    }
    if is_basic_literal(n) {
        return true;
    }
    // Composite literals: array/hash/range/regexp — descend.
    match n {
        Node::ArrayNode { .. } => {
            let arr = n.as_array_node().unwrap();
            arr.elements().iter().all(|e| all_literal_descendants(&e, ignore_constants))
        }
        Node::HashNode { .. } => {
            let h = n.as_hash_node().unwrap();
            h.elements().iter().all(|e| all_literal_descendants(&e, ignore_constants))
        }
        Node::RangeNode { .. } => {
            let r = n.as_range_node().unwrap();
            let l_ok = r.left().map(|x| all_literal_descendants(&x, ignore_constants)).unwrap_or(true);
            let r_ok = r.right().map(|x| all_literal_descendants(&x, ignore_constants)).unwrap_or(true);
            l_ok && r_ok
        }
        Node::RegularExpressionNode { .. } => true, // simple regexp
        Node::InterpolatedRegularExpressionNode { .. } => false,
        Node::InterpolatedStringNode { .. } | Node::InterpolatedSymbolNode { .. } => false,
        _ => false,
    }
}

/// Returns true if descending into `n` we only find basic literals (or pair / const if ignored).
fn all_literal_descendants(n: &Node, ignore_constants: bool) -> bool {
    if is_basic_literal(n) { return true; }
    if ignore_constants && matches!(n, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }) {
        return true;
    }
    match n {
        Node::ArrayNode { .. } => n.as_array_node().unwrap().elements().iter()
            .all(|e| all_literal_descendants(&e, ignore_constants)),
        Node::HashNode { .. } => n.as_hash_node().unwrap().elements().iter()
            .all(|e| all_literal_descendants(&e, ignore_constants)),
        Node::AssocNode { .. } => {
            let a = n.as_assoc_node().unwrap();
            all_literal_descendants(&a.key(), ignore_constants)
                && all_literal_descendants(&a.value(), ignore_constants)
        }
        Node::RangeNode { .. } => {
            let r = n.as_range_node().unwrap();
            r.left().map(|x| all_literal_descendants(&x, ignore_constants)).unwrap_or(true)
                && r.right().map(|x| all_literal_descendants(&x, ignore_constants)).unwrap_or(true)
        }
        Node::RegularExpressionNode { .. } => true,
        _ => false,
    }
}

fn is_basic_literal(n: &Node) -> bool {
    matches!(n,
        Node::IntegerNode { .. } | Node::FloatNode { .. }
        | Node::RationalNode { .. } | Node::ImaginaryNode { .. }
        | Node::TrueNode { .. } | Node::FalseNode { .. } | Node::NilNode { .. }
        | Node::StringNode { .. } | Node::SymbolNode { .. }
        | Node::SourceFileNode { .. } | Node::SourceLineNode { .. } | Node::SourceEncodingNode { .. }
    )
}

fn is_constant_node(n: &Node) -> bool {
    matches!(n, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })
}

impl<'a> BranchVisitor<'a> {
    fn consider_branch(&self, branch: &Branch, total_branches: usize, idx: usize, has_else: bool) -> bool {
        if let Some(body) = &branch.body_node {
            if self.cop.ignore_literal_branches && is_literal_body(body, self.cop.ignore_constant_branches) {
                return false;
            }
            if self.cop.ignore_constant_branches && is_constant_node(body) {
                return false;
            }
        }
        if self.cop.ignore_duplicate_else_branch && has_else
            && total_branches > 2 && idx == total_branches - 1
            && matches!(branch.kind, BranchKind::ElseBranch { .. } | BranchKind::IfBranch { is_else: true, .. })
        {
            return false;
        }
        true
    }

    fn check_branches(&mut self, branches: Vec<Branch<'a>>, has_else: bool) {
        let total = branches.len();
        let mut seen: Vec<String> = Vec::new();
        for (idx, branch) in branches.iter().enumerate() {
            let body_src = match &branch.body_src {
                Some(s) => s,
                None => continue, // empty branch — skip
            };
            if !self.consider_branch(branch, total, idx, has_else) {
                // Don't report; but RuboCop also adds the body to seen via Set.add?
                // We deliberately skip seen.push — `next unless consider_branch?` then Set#add?
                // means a non-considered branch is NOT added to `previous`. Match that.
                continue;
            }
            if seen.contains(body_src) {
                let (s, e) = branch.offense_range();
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/DuplicateBranch",
                    "Duplicate branch body detected.",
                    Severity::Warning,
                    s, e,
                ));
            } else {
                seen.push(body_src.clone());
            }
        }
    }

    /// Process if/unless/elsif → branches list. Skips modifiers and empty (no body) ifs.
    fn process_if(&mut self, node: &ruby_prism::IfNode<'a>) {
        let source = self.ctx.source;
        if is_elsif(node, source) {
            // Only check from outermost; elsif handled via process_if recursion from the outer.
            return;
        }
        // Skip modifier
        let if_loc = node.location();
        let body_first_offset = node.statements().as_ref()
            .and_then(|s| s.body().iter().next())
            .map(|n| n.location().start_offset());
        if is_modifier(if_loc.start_offset(), body_first_offset) {
            return;
        }

        let ternary = is_ternary(node, source);
        let mut branches: Vec<Branch> = Vec::new();
        let mut has_else = false;

        // First branch: the IF body. Parent for offense_range is the outer IfNode itself.
        let outer_loc = node.location();
        let kind = if ternary {
            BranchKind::TernaryBody { start: 0, end: 0 }
        } else {
            BranchKind::IfBranch {
                parent_start: outer_loc.start_offset(),
                parent_end: outer_loc.end_offset(),
                is_else: false,
                is_ternary: false,
            }
        };

        // Build first branch (then branch)
        let then_stmts = node.statements();
        if ternary {
            // Single-expr body
            if let Some(body) = body_single_node(&then_stmts) {
                let loc = body.location();
                branches.push(Branch {
                    body_src: Some(source[loc.start_offset()..loc.end_offset()].to_string()),
                    body_node: Some(body),
                    kind: BranchKind::TernaryBody { start: loc.start_offset(), end: loc.end_offset() },
                });
            }
        } else {
            branches.push(Branch {
                body_src: body_text(&then_stmts, source),
                body_node: body_single_node(&then_stmts),
                kind,
            });
        }

        // Walk subsequent chain
        let mut current = node.subsequent();
        loop {
            match current {
                None => break, // no else: still check whatever branches we've collected
                Some(sub) => {
                    if let Some(elsif_node) = sub.as_if_node() {
                        // elsif branch — parent is this elsif IfNode
                        let elsif_loc = elsif_node.location();
                        let stmts = elsif_node.statements();
                        branches.push(Branch {
                            body_src: body_text(&stmts, source),
                            body_node: body_single_node(&stmts),
                            kind: BranchKind::IfBranch {
                                parent_start: elsif_loc.start_offset(),
                                parent_end: elsif_loc.end_offset(),
                                is_else: false,
                                is_ternary: false,
                            },
                        });
                        current = elsif_node.subsequent();
                    } else if let Some(else_node) = sub.as_else_node() {
                        has_else = true;
                        let else_kw = else_node.else_keyword_loc();
                        let stmts = else_node.statements();
                        if ternary {
                            // Ternary else — single expr
                            if let Some(body) = body_single_node(&stmts) {
                                let loc = body.location();
                                branches.push(Branch {
                                    body_src: Some(source[loc.start_offset()..loc.end_offset()].to_string()),
                                    body_node: Some(body),
                                    kind: BranchKind::TernaryBody { start: loc.start_offset(), end: loc.end_offset() },
                                });
                            }
                        } else {
                            branches.push(Branch {
                                body_src: body_text(&stmts, source),
                                body_node: body_single_node(&stmts),
                                kind: BranchKind::ElseBranch {
                                    start: else_kw.start_offset(),
                                    end: else_kw.end_offset(),
                                },
                            });
                        }
                        break;
                    } else {
                        break;
                    }
                }
            }
        }

        self.check_branches(branches, has_else);
    }

    fn process_unless(&mut self, node: &ruby_prism::UnlessNode<'a>) {
        let source = self.ctx.source;
        // Skip modifier unless
        let body_first = node.statements().as_ref()
            .and_then(|s| s.body().iter().next())
            .map(|n| n.location().start_offset());
        let unless_start = node.location().start_offset();
        if is_modifier(unless_start, body_first) {
            return;
        }

        let unless_loc = node.location();
        let then_stmts = node.statements();
        let mut branches: Vec<Branch> = Vec::new();
        branches.push(Branch {
            body_src: body_text(&then_stmts, source),
            body_node: body_single_node(&then_stmts),
            kind: BranchKind::IfBranch {
                parent_start: unless_loc.start_offset(),
                parent_end: unless_loc.end_offset(),
                is_else: false,
                is_ternary: false,
            },
        });

        let mut has_else = false;
        if let Some(else_node) = node.else_clause() {
            has_else = true;
            let else_kw = else_node.else_keyword_loc();
            let stmts = else_node.statements();
            branches.push(Branch {
                body_src: body_text(&stmts, source),
                body_node: body_single_node(&stmts),
                kind: BranchKind::ElseBranch {
                    start: else_kw.start_offset(),
                    end: else_kw.end_offset(),
                },
            });
        } else {
            return;
        }

        self.check_branches(branches, has_else);
    }

    fn process_case(&mut self, node: &ruby_prism::CaseNode<'a>) {
        let source = self.ctx.source;
        let mut branches: Vec<Branch> = Vec::new();
        for cond in node.conditions().iter() {
            if let Some(when_node) = cond.as_when_node() {
                let when_loc = when_node.location();
                let stmts = when_node.statements();
                branches.push(Branch {
                    body_src: body_text(&stmts, source),
                    body_node: body_single_node(&stmts),
                    kind: BranchKind::WhenBranch {
                        start: when_loc.start_offset(),
                        end: when_loc.end_offset(),
                    },
                });
            }
        }
        let mut has_else = false;
        if let Some(else_node) = node.else_clause() {
            has_else = true;
            let else_kw = else_node.else_keyword_loc();
            let stmts = else_node.statements();
            branches.push(Branch {
                body_src: body_text(&stmts, source),
                body_node: body_single_node(&stmts),
                kind: BranchKind::ElseBranch {
                    start: else_kw.start_offset(),
                    end: else_kw.end_offset(),
                },
            });
        }
        self.check_branches(branches, has_else);
    }

    fn process_case_match(&mut self, node: &ruby_prism::CaseMatchNode<'a>) {
        let source = self.ctx.source;
        let mut branches: Vec<Branch> = Vec::new();
        for cond in node.conditions().iter() {
            if let Some(in_node) = cond.as_in_node() {
                let in_loc = in_node.location();
                let stmts = in_node.statements();
                branches.push(Branch {
                    body_src: body_text(&stmts, source),
                    body_node: body_single_node(&stmts),
                    kind: BranchKind::WhenBranch {
                        start: in_loc.start_offset(),
                        end: in_loc.end_offset(),
                    },
                });
            }
        }
        let mut has_else = false;
        if let Some(else_node) = node.else_clause() {
            has_else = true;
            let else_kw = else_node.else_keyword_loc();
            let stmts = else_node.statements();
            branches.push(Branch {
                body_src: body_text(&stmts, source),
                body_node: body_single_node(&stmts),
                kind: BranchKind::ElseBranch {
                    start: else_kw.start_offset(),
                    end: else_kw.end_offset(),
                },
            });
        }
        self.check_branches(branches, has_else);
    }

    fn process_rescue_chain(&mut self, first: &ruby_prism::RescueNode<'a>, parent: &ruby_prism::BeginNode<'a>) {
        let source = self.ctx.source;
        let mut branches: Vec<Branch> = Vec::new();

        // Walk rescue chain
        let mut cur: Option<ruby_prism::RescueNode> = Some(unsafe { std::ptr::read(first) });
        while let Some(rescue) = cur {
            let r_loc = rescue.location();
            let stmts = rescue.statements();
            branches.push(Branch {
                body_src: body_text(&stmts, source),
                body_node: body_single_node(&stmts),
                kind: BranchKind::RescueBranch {
                    start: r_loc.start_offset(),
                    end: r_loc.end_offset(),
                },
            });
            cur = rescue.subsequent();
        }

        let mut has_else = false;
        if let Some(else_node) = parent.else_clause() {
            has_else = true;
            let else_kw = else_node.else_keyword_loc();
            let stmts = else_node.statements();
            branches.push(Branch {
                body_src: body_text(&stmts, source),
                body_node: body_single_node(&stmts),
                kind: BranchKind::ElseBranch {
                    start: else_kw.start_offset(),
                    end: else_kw.end_offset(),
                },
            });
        }

        self.check_branches(branches, has_else);
    }
}

impl<'a> Visit<'_> for BranchVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.process_if(unsafe { &*(node as *const _ as *const ruby_prism::IfNode<'a>) });
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.process_unless(unsafe { &*(node as *const _ as *const ruby_prism::UnlessNode<'a>) });
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        self.process_case(unsafe { &*(node as *const _ as *const ruby_prism::CaseNode<'a>) });
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        self.process_case_match(unsafe { &*(node as *const _ as *const ruby_prism::CaseMatchNode<'a>) });
        ruby_prism::visit_case_match_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        let n = unsafe { &*(node as *const _ as *const ruby_prism::BeginNode<'a>) };
        if let Some(rescue) = n.rescue_clause() {
            self.process_rescue_chain(&rescue, n);
        }
        ruby_prism::visit_begin_node(self, node);
    }
}

// Suppress warnings for helpers not used in non-config tests
#[allow(dead_code)]
fn _suppress(n: &Node, s: &str) -> String { node_to_text(n, s) }

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    ignore_literal_branches: bool,
    ignore_constant_branches: bool,
    ignore_duplicate_else_branch: bool,
}

crate::register_cop!("Lint/DuplicateBranch", |cfg| {
    let c: Cfg = cfg.typed("Lint/DuplicateBranch");
    Some(Box::new(DuplicateBranch::with_config(
        c.ignore_literal_branches, c.ignore_constant_branches, c.ignore_duplicate_else_branch
    )))
});
