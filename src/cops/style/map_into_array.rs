//! Style/MapIntoArray — replace `each { ... << ... }` with `map`.
//!
//! Flags the pattern:
//!     dest = []
//!     src.each { |e| dest << f(e) }
//! and autocorrects to:
//!     dest = src.map { |e| f(e) }
//!
//! Also handles the `[].tap { |dest| src.each { |e| dest << f(e) } }` form.
//!
//! Ported from `rubocop/lib/rubocop/cop/style/map_into_array.rb`.
//!
//! Cross-statement ref tracking uses a parent-chain of frame metadata
//! (Prism nodes are not Clone) + on-demand re-parse scans for assignment /
//! reference counting — matching RuboCop's `dest_used_only_for_mapping?`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct MapIntoArray {
    preferred_method: String,
}

impl Default for MapIntoArray {
    fn default() -> Self {
        Self {
            preferred_method: "map".to_string(),
        }
    }
}

impl MapIntoArray {
    pub fn new(preferred_method: String) -> Self {
        Self { preferred_method }
    }
}

impl Cop for MapIntoArray {
    fn name(&self) -> &'static str {
        "Style/MapIntoArray"
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Walker {
            ctx,
            preferred_method: &self.preferred_method,
            offenses: Vec::new(),
            parent_stack: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

// ── Frame types (no Prism-node storage since they're not Clone) ──

#[derive(Clone)]
struct Frame {
    kind: FrameKind,
    start: usize,
    end: usize,
    /// Starting offsets of the frame's direct child statements (for parents with such a list).
    children: Vec<usize>,
    /// Extra metadata. Def: method name. Block: call's method name.
    extra: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FrameKind {
    Program,
    Begin,
    Block,
    Def,
    Lambda,
    Ensure,
    Parentheses,
    If,
    Unless,
    While,
    Until,
    For,
    Case,
    Other,
}

struct Walker<'a, 'b> {
    ctx: &'a CheckContext<'a>,
    preferred_method: &'b str,
    offenses: Vec<Offense>,
    parent_stack: Vec<Frame>,
}

struct Candidate {
    block_start: usize,
    block_end: usize,
    each_send_start: usize,
    each_send_end: usize,
    selector_start: usize,
    selector_end: usize,
    dest_name: String,
    push_start: usize,
    push_end: usize,
    arg_start: usize,
    arg_end: usize,
    arg_is_unbraced_hash: bool,
}

impl<'a, 'b> Walker<'a, 'b> {
    fn handle_call_with_block(
        &mut self,
        send: &ruby_prism::CallNode<'a>,
        block: &ruby_prism::BlockNode<'a>,
    ) {
        let method = node_name!(send);
        if method != "each" {
            return;
        }
        let recv = match send.receiver() {
            Some(r) => r,
            None => return,
        };
        if recv.as_self_node().is_some() {
            return;
        }
        if let Some(args) = send.arguments() {
            if args.arguments().iter().count() > 0 {
                return;
            }
        }

        let body_opt = block.body();
        let body = match body_opt {
            Some(b) => b,
            None => return,
        };
        let push_call_node = match single_statement(&body) {
            Some(n) => n,
            None => return,
        };
        let push_call = match push_call_node.as_call_node() {
            Some(c) => c,
            None => return,
        };
        let push_method = node_name!(push_call);
        if push_method != "<<" && push_method != "push" && push_method != "append" {
            return;
        }
        let push_recv = match push_call.receiver() {
            Some(r) => r,
            None => return,
        };
        let dest_read = match push_recv.as_local_variable_read_node() {
            Some(lv) => lv,
            None => return,
        };
        let dest_name = String::from_utf8_lossy(dest_read.name().as_slice()).to_string();
        let push_args = match push_call.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_nodes: Vec<Node> = push_args.arguments().iter().collect();
        if arg_nodes.len() != 1 {
            return;
        }
        let arg = &arg_nodes[0];
        if !is_suitable_argument(arg) {
            return;
        }
        if block_params_shadow(block, &dest_name) {
            return;
        }
        if var_referenced_in(&recv, &dest_name) {
            return;
        }
        if var_referenced_in(arg, &dest_name) {
            return;
        }

        let arg_is_unbraced_hash = match arg {
            Node::KeywordHashNode { .. } => true,
            Node::HashNode { .. } => {
                let h = arg.as_hash_node().unwrap();
                let ol = h.opening_loc();
                let src = self.ctx.source.as_bytes();
                let byte = src.get(ol.start_offset()).copied().unwrap_or(0);
                byte != b'{'
            }
            _ => false,
        };

        let block_loc = block.location();
        let send_loc = send.location();
        let selector_loc = send.message_loc().expect("each selector");
        let arg_loc = arg.location();
        let push_loc = push_call.location();

        let cand = Candidate {
            block_start: block_loc.start_offset(),
            block_end: block_loc.end_offset(),
            each_send_start: send_loc.start_offset(),
            each_send_end: send_loc.end_offset(),
            selector_start: selector_loc.start_offset(),
            selector_end: selector_loc.end_offset(),
            dest_name,
            push_start: push_loc.start_offset(),
            push_end: push_loc.end_offset(),
            arg_start: arg_loc.start_offset(),
            arg_end: arg_loc.end_offset(),
            arg_is_unbraced_hash,
        };

        self.check_candidate(cand);
    }

    fn check_candidate(&mut self, cand: Candidate) {
        // The `each` call (not its block) is the "expression" that's a sibling in the
        // enclosing statement group. Use each_send_start as the sibling key.
        let sibling_key = cand.each_send_start;

        // Path A: tap parent.
        if let Some((tap_idx, tap_call_start)) = self.tap_parent_idx(sibling_key, &cand.dest_name) {
            self.emit_tap(cand, tap_idx, tap_call_start);
            return;
        }

        // Path B: sibling-group parent.
        let (frame_idx, siblings) = match self.sibling_group_for(sibling_key) {
            Some(s) => s,
            None => return,
        };
        let parent_kind = self.parent_stack[frame_idx].kind.clone();
        match parent_kind {
            FrameKind::Program
            | FrameKind::Begin
            | FrameKind::Block
            | FrameKind::Parentheses
            | FrameKind::Def
            | FrameKind::Lambda
            | FrameKind::Ensure
            | FrameKind::For => {}
            _ => return,
        }

        let mut asgn_start: Option<usize> = None;
        let mut asgn_end: Option<usize> = None;
        for sib_off in &siblings {
            if *sib_off >= sibling_key {
                break;
            }
            if let Some((name, val_start, end)) =
                find_top_level_lvar_write(self.ctx.source, *sib_off)
            {
                if name == cand.dest_name {
                    if is_empty_array_value_by_offsets(self.ctx.source, val_start) {
                        asgn_start = Some(*sib_off);
                        asgn_end = Some(end);
                    } else {
                        asgn_start = None;
                        asgn_end = None;
                    }
                }
            }
        }
        let (asgn_start, asgn_end) = match (asgn_start, asgn_end) {
            (Some(s), Some(e)) => (s, e),
            _ => return,
        };

        let (reads, writes) = count_refs_in_range(
            self.ctx.source,
            asgn_start,
            cand.block_end,
            &cand.dest_name,
        );
        if reads != 1 || writes != 1 {
            return;
        }

        self.emit_normal(cand, asgn_start, asgn_end, &siblings, parent_kind, frame_idx, sibling_key);
    }

    fn sibling_group_for(&self, sibling_key: usize) -> Option<(usize, Vec<usize>)> {
        for (i, f) in self.parent_stack.iter().enumerate().rev() {
            match f.kind {
                FrameKind::Program
                | FrameKind::Begin
                | FrameKind::Block
                | FrameKind::Def
                | FrameKind::Lambda
                | FrameKind::Ensure
                | FrameKind::Parentheses
                | FrameKind::For => {
                    if f.children.contains(&sibling_key) {
                        return Some((i, f.children.clone()));
                    }
                }
                FrameKind::Other => return None,
                _ => return None,
            }
        }
        None
    }

    fn tap_parent_idx(&self, each_call_start: usize, dest_name: &str) -> Option<(usize, usize)> {
        for (i, f) in self.parent_stack.iter().enumerate().rev() {
            if f.kind == FrameKind::Block {
                if f.children.len() == 1 && f.children[0] == each_call_start && f.extra == "tap" {
                    if let Some(call_start) = tap_block_matches(self.ctx.source, f.start, f.end, dest_name) {
                        return Some((i, call_start));
                    }
                }
                return None;
            }
        }
        None
    }

    // ── Emit ──

    fn message(&self) -> String {
        format!(
            "Use `{}` instead of `each` to map elements into an array.",
            self.preferred_method
        )
    }

    fn emit_tap(&mut self, cand: Candidate, tap_idx: usize, tap_call_start: usize) {
        let tap = self.parent_stack[tap_idx].clone();
        let off = self.ctx.offense_with_range(
            "Style/MapIntoArray",
            &self.message(),
            Severity::Convention,
            cand.each_send_start,
            cand.each_send_end,
        );
        let edits = self.build_correction_tap(&cand, tap_call_start, tap.end);
        self.offenses.push(off.with_correction(Correction { edits }));
    }

    fn emit_normal(
        &mut self,
        cand: Candidate,
        asgn_start: usize,
        asgn_end: usize,
        siblings: &[usize],
        parent_kind: FrameKind,
        frame_idx: usize,
        sibling_key: usize,
    ) {
        let off = self.ctx.offense_with_range(
            "Style/MapIntoArray",
            &self.message(),
            Severity::Convention,
            cand.each_send_start,
            cand.each_send_end,
        );

        let is_last = siblings.last().map_or(false, |o| *o == sibling_key);
        let return_used = is_last && self.parent_return_used(parent_kind, frame_idx);
        if return_used {
            self.offenses.push(off);
            return;
        }

        let edits = self.build_correction_normal(&cand, asgn_start, asgn_end, siblings, sibling_key);
        let _ = asgn_end;
        self.offenses.push(off.with_correction(Correction { edits }));
    }

    fn parent_return_used(&self, kind: FrameKind, frame_idx: usize) -> bool {
        match kind {
            FrameKind::Program => false,
            FrameKind::Begin | FrameKind::Parentheses | FrameKind::Ensure => {
                if frame_idx == 0 {
                    return false;
                }
                let this_start = self.parent_stack[frame_idx].start;
                for i in (0..frame_idx).rev() {
                    let f = &self.parent_stack[i];
                    match f.kind {
                        FrameKind::Program
                        | FrameKind::Begin
                        | FrameKind::Block
                        | FrameKind::Def
                        | FrameKind::Lambda
                        | FrameKind::Ensure
                        | FrameKind::Parentheses => {
                            if f.children.contains(&this_start) {
                                let is_last = f.children.last() == Some(&this_start);
                                if !is_last {
                                    return false;
                                }
                                return self.parent_return_used(f.kind.clone(), i);
                            }
                        }
                        _ => return true,
                    }
                }
                false
            }
            FrameKind::Def => {
                let name = &self.parent_stack[frame_idx].extra;
                if name == "initialize" || name.ends_with('=') {
                    return false;
                }
                true
            }
            FrameKind::Block => {
                let m = &self.parent_stack[frame_idx].extra;
                if matches!(
                    m.as_str(),
                    "each" | "each_with_index" | "tap" | "loop" | "times" | "reverse_each"
                ) {
                    return false;
                }
                true
            }
            FrameKind::Lambda => true,
            FrameKind::If
            | FrameKind::Unless
            | FrameKind::While
            | FrameKind::Until
            | FrameKind::For => false,
            FrameKind::Case => true,
            FrameKind::Other => true,
        }
    }

    // ── Corrections ──

    fn build_correction_tap(&self, cand: &Candidate, tap_start: usize, tap_end: usize) -> Vec<Edit> {
        let mut edits = Vec::new();
        edits.push(Edit {
            start_offset: cand.selector_start,
            end_offset: cand.selector_end,
            replacement: self.preferred_method.to_string(),
        });
        edits.push(Edit {
            start_offset: tap_start,
            end_offset: cand.each_send_start,
            replacement: format!("{} = ", cand.dest_name),
        });
        edits.push(Edit {
            start_offset: cand.block_end,
            end_offset: tap_end,
            replacement: String::new(),
        });
        self.emit_push_edits(cand, &mut edits);
        edits
    }

    fn build_correction_normal(
        &self,
        cand: &Candidate,
        asgn_start: usize,
        asgn_end: usize,
        siblings: &[usize],
        sibling_key: usize,
    ) -> Vec<Edit> {
        let mut edits = Vec::new();

        edits.push(Edit {
            start_offset: cand.selector_start,
            end_offset: cand.selector_end,
            replacement: self.preferred_method.to_string(),
        });

        let src = self.ctx.source.as_bytes();
        let removal_end = {
            let mut i = asgn_end;
            while i < src.len() && (src[i] == b' ' || src[i] == b'\t') {
                i += 1;
            }
            if i < src.len() && src[i] == b'\n' {
                i += 1;
            }
            while i < src.len() && (src[i] == b' ' || src[i] == b'\t') {
                i += 1;
            }
            i
        };
        edits.push(Edit {
            start_offset: asgn_start,
            end_offset: removal_end,
            replacement: String::new(),
        });

        edits.push(Edit {
            start_offset: cand.each_send_start,
            end_offset: cand.each_send_start,
            replacement: format!("{} = ", cand.dest_name),
        });

        self.emit_push_edits(cand, &mut edits);

        if let Some(idx) = siblings.iter().position(|&o| o == sibling_key) {
            if idx + 1 < siblings.len() {
                let next_start = siblings[idx + 1];
                if let Some((name, end_off)) =
                    find_top_level_lvar_read(self.ctx.source, next_start)
                {
                    if name == cand.dest_name {
                        let mut s = next_start;
                        while s > cand.block_end
                            && matches!(src[s - 1], b' ' | b'\t' | b'\n')
                        {
                            s -= 1;
                        }
                        edits.push(Edit {
                            start_offset: s,
                            end_offset: end_off,
                            replacement: String::new(),
                        });
                    }
                }
            }
        }

        edits
    }

    fn emit_push_edits(&self, cand: &Candidate, edits: &mut Vec<Edit>) {
        if cand.arg_is_unbraced_hash {
            edits.push(Edit {
                start_offset: cand.arg_start,
                end_offset: cand.arg_start,
                replacement: "{ ".to_string(),
            });
            edits.push(Edit {
                start_offset: cand.arg_end,
                end_offset: cand.arg_end,
                replacement: " }".to_string(),
            });
        }
        edits.push(Edit {
            start_offset: cand.push_start,
            end_offset: cand.arg_start,
            replacement: String::new(),
        });
        edits.push(Edit {
            start_offset: cand.arg_end,
            end_offset: cand.push_end,
            replacement: String::new(),
        });
    }
}

// ── Free helpers ──

fn single_statement<'a>(body: &Node<'a>) -> Option<Node<'a>> {
    if let Some(stmts) = body.as_statements_node() {
        let list: Vec<Node<'a>> = stmts.body().iter().collect();
        if list.len() == 1 {
            return Some(list.into_iter().next().unwrap());
        }
        return None;
    }
    None
}

fn block_params_shadow(block: &ruby_prism::BlockNode, dest_name: &str) -> bool {
    let params = match block.parameters() {
        Some(p) => p,
        None => return false,
    };
    let pn = match params.as_block_parameters_node() {
        Some(p) => p,
        None => return false,
    };
    if let Some(ps) = pn.parameters() {
        for r in ps.requireds().iter() {
            if let Some(req) = r.as_required_parameter_node() {
                let n = String::from_utf8_lossy(req.name().as_slice());
                if n == dest_name {
                    return true;
                }
            }
        }
    }
    false
}

fn var_referenced_in(node: &Node, name: &str) -> bool {
    let mut f = RefCounter {
        name,
        reads: 0,
        writes: 0,
    };
    f.visit(node);
    f.reads + f.writes > 0
}

struct RefCounter<'n> {
    name: &'n str,
    reads: usize,
    writes: usize,
}

impl<'n> Visit<'_> for RefCounter<'n> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name {
            self.reads += 1;
        }
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name {
            self.writes += 1;
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode,
    ) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name {
            self.writes += 1;
        }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableAndWriteNode,
    ) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name {
            self.writes += 1;
        }
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOrWriteNode,
    ) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name {
            self.writes += 1;
        }
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
}

fn is_suitable_argument(arg: &Node) -> bool {
    match arg {
        Node::SplatNode { .. } => false,
        Node::ForwardingArgumentsNode { .. } => false,
        Node::BlockArgumentNode { .. } => {
            let ba = arg.as_block_argument_node().unwrap();
            ba.expression().is_some()
        }
        _ => {
            if let Some(h) = arg.as_hash_node() {
                let elements: Vec<_> = h.elements().iter().collect();
                if elements.len() == 1 {
                    if let Some(assoc_splat) = elements[0].as_assoc_splat_node() {
                        if assoc_splat.value().is_none() {
                            return false;
                        }
                    }
                }
            }
            if let Some(kh) = arg.as_keyword_hash_node() {
                let elements: Vec<_> = kh.elements().iter().collect();
                if elements.len() == 1 {
                    if let Some(assoc_splat) = elements[0].as_assoc_splat_node() {
                        if assoc_splat.value().is_none() {
                            return false;
                        }
                    }
                }
            }
            true
        }
    }
}

fn find_top_level_lvar_write(source: &str, start: usize) -> Option<(String, usize, usize)> {
    let res = ruby_prism::parse(source.as_bytes());
    let root = res.node();
    let mut finder = LvarWriteAtFinder {
        target: start,
        result: None,
    };
    finder.visit(&root);
    finder.result
}

struct LvarWriteAtFinder {
    target: usize,
    result: Option<(String, usize, usize)>,
}

impl Visit<'_> for LvarWriteAtFinder {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        if node.location().start_offset() == self.target && self.result.is_none() {
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            let v = node.value();
            let val_start = v.location().start_offset();
            let end = node.location().end_offset();
            self.result = Some((name, val_start, end));
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
}

fn find_top_level_lvar_read(source: &str, start: usize) -> Option<(String, usize)> {
    let res = ruby_prism::parse(source.as_bytes());
    let root = res.node();
    let mut finder = LvarReadAtFinder {
        target: start,
        result: None,
    };
    finder.visit(&root);
    finder.result
}

struct LvarReadAtFinder {
    target: usize,
    result: Option<(String, usize)>,
}

impl Visit<'_> for LvarReadAtFinder {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        if node.location().start_offset() == self.target && self.result.is_none() {
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            self.result = Some((name, node.location().end_offset()));
        }
    }
}

fn is_empty_array_value_by_offsets(source: &str, val_start: usize) -> bool {
    let res = ruby_prism::parse(source.as_bytes());
    let root = res.node();
    let mut finder = ValueAtFinder {
        target: val_start,
        is_empty: false,
    };
    finder.visit(&root);
    finder.is_empty
}

struct ValueAtFinder {
    target: usize,
    is_empty: bool,
}

impl Visit<'_> for ValueAtFinder {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let v = node.value();
        if v.location().start_offset() == self.target {
            self.is_empty = is_empty_array_node(&v);
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
}

fn is_empty_array_node(val: &Node) -> bool {
    if let Some(arr) = val.as_array_node() {
        return arr.elements().iter().count() == 0;
    }
    if let Some(call) = val.as_call_node() {
        let m = node_name!(call);
        if m == "new" {
            if let Some(recv) = call.receiver() {
                if let Some(c) = recv.as_constant_read_node() {
                    if node_name!(c) == "Array" {
                        if let Some(args) = call.arguments() {
                            let arg_list: Vec<_> = args.arguments().iter().collect();
                            if arg_list.is_empty() {
                                return true;
                            }
                            if arg_list.len() == 1 {
                                if let Some(a) = arg_list[0].as_array_node() {
                                    return a.elements().iter().count() == 0;
                                }
                            }
                            return false;
                        } else {
                            return true;
                        }
                    }
                }
            }
        }
        if m == "[]" {
            if let Some(recv) = call.receiver() {
                if let Some(c) = recv.as_constant_read_node() {
                    if node_name!(c) == "Array" {
                        if let Some(args) = call.arguments() {
                            return args.arguments().iter().count() == 0;
                        }
                        return true;
                    }
                }
            }
        }
        if m == "Array" && call.receiver().is_none() {
            if let Some(args) = call.arguments() {
                let al: Vec<_> = args.arguments().iter().collect();
                if al.len() == 1 {
                    if let Some(a) = al[0].as_array_node() {
                        return a.elements().iter().count() == 0;
                    }
                }
            }
        }
    }
    false
}

fn count_refs_in_range(source: &str, start: usize, end: usize, name: &str) -> (usize, usize) {
    let res = ruby_prism::parse(source.as_bytes());
    let root = res.node();
    let mut counter = RangedCounter {
        name,
        range_start: start,
        range_end: end,
        reads: 0,
        writes: 0,
    };
    counter.visit(&root);
    (counter.reads, counter.writes)
}

struct RangedCounter<'a> {
    name: &'a str,
    range_start: usize,
    range_end: usize,
    reads: usize,
    writes: usize,
}

impl<'a> RangedCounter<'a> {
    fn in_range(&self, offset: usize) -> bool {
        offset >= self.range_start && offset < self.range_end
    }
}

impl<'a> Visit<'_> for RangedCounter<'a> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name && self.in_range(node.location().start_offset()) {
            self.reads += 1;
        }
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name && self.in_range(node.location().start_offset()) {
            self.writes += 1;
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode,
    ) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n == self.name && self.in_range(node.location().start_offset()) {
            self.writes += 1;
        }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
}

fn tap_block_matches(source: &str, start: usize, end: usize, dest_name: &str) -> Option<usize> {
    let res = ruby_prism::parse(source.as_bytes());
    let root = res.node();
    let mut call_start: Option<usize> = None;
    let mut finder = TapMatchFinder {
        target_start: start,
        target_end: end,
        dest_name,
        call_start: &mut call_start,
    };
    finder.visit(&root);
    call_start
}

struct TapMatchFinder<'a> {
    target_start: usize,
    target_end: usize,
    dest_name: &'a str,
    call_start: &'a mut Option<usize>,
}

impl<'a> Visit<'_> for TapMatchFinder<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if node_name!(node) == "tap" {
            if let Some(block_node) = node.block() {
                if let Some(block) = block_node.as_block_node() {
                    let loc = block.location();
                    if loc.start_offset() == self.target_start && loc.end_offset() == self.target_end {
                        if let Some(recv) = node.receiver() {
                            if let Some(arr) = recv.as_array_node() {
                                if arr.elements().iter().count() == 0 {
                                    if let Some(params) = block.parameters() {
                                        if let Some(pn) = params.as_block_parameters_node() {
                                            if let Some(ps) = pn.parameters() {
                                                let req: Vec<_> = ps.requireds().iter().collect();
                                                if req.len() == 1 {
                                                    if let Some(arg) =
                                                        req[0].as_required_parameter_node()
                                                    {
                                                        let an = String::from_utf8_lossy(
                                                            arg.name().as_slice(),
                                                        );
                                                        if an == self.dest_name {
                                                            *self.call_start = Some(node.location().start_offset());
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
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

// ── Visitor: push frames + dispatch handle_call_with_block ──

fn gather_statement_offsets(body: Option<&Node>) -> Vec<usize> {
    match body {
        Some(n) => {
            if let Some(stmts) = n.as_statements_node() {
                stmts
                    .body()
                    .iter()
                    .map(|s| s.location().start_offset())
                    .collect()
            } else {
                vec![n.location().start_offset()]
            }
        }
        None => Vec::new(),
    }
}

impl<'a, 'b> Visit<'a> for Walker<'a, 'b> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode<'a>) {
        let children: Vec<usize> = node
            .statements()
            .body()
            .iter()
            .map(|s| s.location().start_offset())
            .collect();
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Program,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children,
            extra: String::new(),
        });
        ruby_prism::visit_program_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        if let Some(block_node) = node.block() {
            if let Some(block) = block_node.as_block_node() {
                self.handle_call_with_block(node, &block);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        let loc = node.location();
        let method = find_block_parent_method(self.ctx.source, loc.start_offset());

        let body_vec: Vec<Node<'a>> = node.body().into_iter().collect();
        let children = gather_statement_offsets(body_vec.first());

        self.parent_stack.push(Frame {
            kind: FrameKind::Block,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children,
            extra: method,
        });
        ruby_prism::visit_block_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'a>) {
        let children: Vec<usize> = node
            .statements()
            .into_iter()
            .flat_map(|s| {
                s.body()
                    .iter()
                    .map(|n| n.location().start_offset())
                    .collect::<Vec<_>>()
            })
            .collect();
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Begin,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children,
            extra: String::new(),
        });
        ruby_prism::visit_begin_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'a>) {
        let body_vec: Vec<Node<'a>> = node.body().into_iter().collect();
        let children = gather_statement_offsets(body_vec.first());
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Def,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children,
            extra: name,
        });
        ruby_prism::visit_def_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'a>) {
        let body_vec: Vec<Node<'a>> = node.body().into_iter().collect();
        let children = gather_statement_offsets(body_vec.first());
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Lambda,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children,
            extra: String::new(),
        });
        ruby_prism::visit_lambda_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode<'a>) {
        let children: Vec<usize> = node
            .statements()
            .into_iter()
            .flat_map(|s| {
                s.body()
                    .iter()
                    .map(|n| n.location().start_offset())
                    .collect::<Vec<_>>()
            })
            .collect();
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Ensure,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children,
            extra: String::new(),
        });
        ruby_prism::visit_ensure_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode<'a>) {
        let body_vec: Vec<Node<'a>> = node.body().into_iter().collect();
        let children = gather_statement_offsets(body_vec.first());
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Parentheses,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children,
            extra: String::new(),
        });
        ruby_prism::visit_parentheses_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'a>) {
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::If,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children: Vec::new(),
            extra: String::new(),
        });
        ruby_prism::visit_if_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'a>) {
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Unless,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children: Vec::new(),
            extra: String::new(),
        });
        ruby_prism::visit_unless_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'a>) {
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::While,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children: Vec::new(),
            extra: String::new(),
        });
        ruby_prism::visit_while_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'a>) {
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Until,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children: Vec::new(),
            extra: String::new(),
        });
        ruby_prism::visit_until_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode<'a>) {
        let body: Vec<usize> = node
            .statements()
            .into_iter()
            .flat_map(|s| {
                s.body()
                    .iter()
                    .map(|n| n.location().start_offset())
                    .collect::<Vec<_>>()
            })
            .collect();
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::For,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children: body,
            extra: String::new(),
        });
        ruby_prism::visit_for_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'a>) {
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Case,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children: Vec::new(),
            extra: String::new(),
        });
        ruby_prism::visit_case_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'a>) {
        let loc = node.location();
        self.parent_stack.push(Frame {
            kind: FrameKind::Other,
            start: loc.start_offset(),
            end: loc.end_offset(),
            children: Vec::new(),
            extra: String::new(),
        });
        ruby_prism::visit_array_node(self, node);
        self.parent_stack.pop();
    }
}

fn find_block_parent_method(source: &str, block_start: usize) -> String {
    let res = ruby_prism::parse(source.as_bytes());
    let root = res.node();
    let mut finder = BlockParentFinder {
        target: block_start,
        method: String::new(),
    };
    finder.visit(&root);
    finder.method
}

struct BlockParentFinder {
    target: usize,
    method: String,
}

impl Visit<'_> for BlockParentFinder {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(block_node) = node.block() {
            if let Some(block) = block_node.as_block_node() {
                if block.location().start_offset() == self.target && self.method.is_empty() {
                    self.method = node_name!(node).to_string();
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/MapIntoArray", |cfg| {
    let preferred = cfg
        .get_cop_config("Style/CollectionMethods")
        .and_then(|c| c.raw.get("PreferredMethods"))
        .and_then(|v| v.as_mapping())
        .and_then(|m| m.get(serde_yaml::Value::String("map".to_string())))
        .and_then(|v| v.as_str())
        .unwrap_or("map")
        .to_string();
    Some(Box::new(MapIntoArray::new(preferred)))
});
