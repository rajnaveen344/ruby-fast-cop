//! Lint/NonAtomicFileOperation cop
//!
//! Detects `if FileTest.exist?(x); FileUtils.mkdir(x); end` patterns — race
//! conditions between check and action. Suggest atomic alternatives.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MAKE_FORCE_METHODS: &[&str] = &["makedirs", "mkdir_p", "mkpath"];
const MAKE_METHODS: &[&str] = &["mkdir"];
const REMOVE_FORCE_METHODS: &[&str] = &["rm_f", "rm_rf"];
const REMOVE_METHODS: &[&str] = &["remove", "delete", "unlink", "remove_file", "rm", "rmdir", "safe_unlink"];
const RECURSIVE_REMOVE_METHODS: &[&str] = &["remove_dir", "remove_entry", "remove_entry_secure"];

const EXIST_RECEIVERS: &[&str] = &["FileTest", "File", "Dir", "Shell"];
const EXIST_METHODS: &[&str] = &["exist?", "exists?"];

#[derive(Default)]
pub struct NonAtomicFileOperation;

impl NonAtomicFileOperation {
    pub fn new() -> Self {
        Self
    }
}

fn is_target_method(m: &str) -> bool {
    MAKE_METHODS.contains(&m)
        || MAKE_FORCE_METHODS.contains(&m)
        || REMOVE_METHODS.contains(&m)
        || RECURSIVE_REMOVE_METHODS.contains(&m)
        || REMOVE_FORCE_METHODS.contains(&m)
}

fn is_force_method(m: &str) -> bool {
    MAKE_FORCE_METHODS.contains(&m) || REMOVE_FORCE_METHODS.contains(&m)
}

fn replacement_method(m: &str) -> &'static str {
    if MAKE_METHODS.contains(&m) {
        "mkdir_p"
    } else if REMOVE_METHODS.contains(&m) {
        "rm_f"
    } else if RECURSIVE_REMOVE_METHODS.contains(&m) {
        "rm_rf"
    } else if m == "makedirs" {
        "makedirs"
    } else if m == "mkdir_p" {
        "mkdir_p"
    } else if m == "mkpath" {
        "mkpath"
    } else if m == "rm_f" {
        "rm_f"
    } else if m == "rm_rf" {
        "rm_rf"
    } else {
        "" // unreachable for is_target_method
    }
}

/// Walk recursively looking for `force: true/false` kwarg among hash args.
/// Returns Some(true) if force:true found, Some(false) if force:false, None otherwise.
fn check_pairs<'a>(pairs: impl Iterator<Item = Node<'a>>) -> Option<bool> {
    for elem in pairs {
        if let Some(pair) = elem.as_assoc_node() {
            let k = pair.key();
            if let Some(sym) = k.as_symbol_node() {
                if String::from_utf8_lossy(sym.unescaped()) == "force" {
                    let v = pair.value();
                    if v.as_true_node().is_some() {
                        return Some(true);
                    }
                    if v.as_false_node().is_some() {
                        return Some(false);
                    }
                }
            }
        }
    }
    None
}

fn find_force_kwarg(node: &Node) -> Option<bool> {
    if let Some(h) = node.as_hash_node() {
        return check_pairs(h.elements().iter());
    }
    if let Some(k) = node.as_keyword_hash_node() {
        return check_pairs(k.elements().iter());
    }
    None
}

/// Extract exist? info from a node: returns (receiver_name, method_name, first_arg_src, first_arg_node).
fn exist_call_info2<'a, 's>(
    node: &Node<'a>,
    src: &'s str,
) -> Option<(String, String, &'s str, Node<'a>)> {
    let call = node.as_call_node()?;
    let mname = node_name!(call).into_owned();
    if !EXIST_METHODS.contains(&mname.as_str()) {
        return None;
    }
    let recv = call.receiver()?;
    let rname = m::constant_simple_name(&recv)?;
    if !EXIST_RECEIVERS.contains(&rname.as_ref()) {
        return None;
    }
    if !m::is_toplevel_constant_named(&recv, &rname) {
        return None;
    }
    // First arg source.
    let args = call.arguments()?;
    let first = args.arguments().iter().next()?;
    let loc = first.location();
    Some((rname.into_owned(), mname, &src[loc.start_offset()..loc.end_offset()], first))
}

/// Unwrap `!x` and paren-wrapped nodes for condition extraction.
fn unwrap_condition<'a>(node: Node<'a>) -> Node<'a> {
    let mut cur = node;
    loop {
        if let Some(call) = cur.as_call_node() {
            if node_name!(call) == "!" && call.receiver().is_some() && call.arguments().is_none() {
                // unary not
                cur = call.receiver().unwrap();
                continue;
            }
        }
        if let Some(p) = cur.as_parentheses_node() {
            if let Some(body) = p.body() {
                if let Some(stmts) = body.as_statements_node() {
                    let v: Vec<_> = stmts.body().iter().collect();
                    if v.len() == 1 {
                        cur = v.into_iter().next().unwrap();
                        continue;
                    }
                }
            }
        }
        break;
    }
    cur
}

/// Check for `&&`/`||` in condition → allowable.
fn is_logical_keyword<'a>(node: &Node<'a>) -> bool {
    node.as_and_node().is_some() || node.as_or_node().is_some()
}

struct V<'ctx, 's> {
    ctx: &'ctx CheckContext<'s>,
    offenses: Vec<Offense>,
    cop_name: &'static str,
}

fn is_elsif<'a>(if_node: &ruby_prism::IfNode<'a>, src: &str) -> bool {
    let kw = if_node.if_keyword_loc();
    let Some(kw) = kw else { return false };
    let s = &src[kw.start_offset()..kw.end_offset()];
    s == "elsif"
}

impl<'pr, 'ctx, 's: 'pr> Visit<'pr> for V<'ctx, 's> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        self.check_if_like(Some(node), None);
        // Default descent
        ruby_prism::visit_if_node(self, node);
    }
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        self.check_if_like(None, Some(node));
        ruby_prism::visit_unless_node(self, node);
    }
}

impl<'ctx, 's> V<'ctx, 's> {
    fn check_if_like<'a>(
        &mut self,
        if_node: Option<&ruby_prism::IfNode<'a>>,
        unless_node: Option<&ruby_prism::UnlessNode<'a>>,
    ) where
        's: 'a,
    {
        let src = self.ctx.source;

        // Extract: condition, keyword-loc (begin of `if`/`unless`/`elsif`),
        // end-loc (end keyword) if block form, then_body, else_branch, modifier?, elsif?.
        let (cond_node, kw_loc, end_loc_opt, then_body, has_else, is_elsif_branch) = if let Some(ifn) = if_node {
            let cond = ifn.predicate();
            let kw_loc = match ifn.if_keyword_loc() { Some(l) => l, None => return };
            let end_loc = ifn.end_keyword_loc();
            let then_body = ifn.statements();
            let has_else = ifn.subsequent().is_some();
            let is_elsif = is_elsif(ifn, src);
            (cond, kw_loc, end_loc, then_body, has_else, is_elsif)
        } else if let Some(un) = unless_node {
            let cond = un.predicate();
            let kw_loc = un.keyword_loc();
            let end_loc = un.end_keyword_loc();
            let then_body = un.statements();
            let has_else = un.else_clause().is_some();
            (cond, kw_loc, end_loc, then_body, has_else, false)
        } else {
            return;
        };

        // Disallowed: has else branch (allowable_use_with_if) OR condition is
        // logical keyword (&&/||).
        if has_else {
            return;
        }
        if is_logical_keyword(&cond_node) {
            return;
        }
        let cond_loc_saved = cond_node.location();

        // Extract exist? from condition (after unwrapping `!` and parens).
        let cond_inner = unwrap_condition(cond_node);
        let exist_info = match exist_call_info2(&cond_inner, src) {
            Some(info) => info,
            None => return,
        };
        let (exist_receiver, exist_method, exist_arg_src, _exist_arg_node) = exist_info;

        // Find the FileUtils-style call in `then_body`.
        let stmts = match then_body {
            Some(s) => s,
            None => return,
        };
        let body: Vec<Node> = stmts.body().iter().collect();
        if body.len() != 1 {
            return;
        }
        let stmt = &body[0];
        let target_call = match stmt.as_call_node() {
            Some(c) => c,
            None => return,
        };
        let target_method = node_name!(target_call).into_owned();
        if !is_target_method(&target_method) {
            return;
        }
        // Receiver must be const.
        let target_recv = match target_call.receiver() {
            Some(r) => r,
            None => return,
        };
        let target_recv_name = match m::constant_simple_name(&target_recv) {
            Some(n) => n,
            None => return,
        };
        // First arg of target must match first arg of exist call (by source).
        let args = match target_call.arguments() {
            Some(a) => a,
            None => return,
        };
        let first_arg = match args.arguments().iter().next() {
            Some(a) => a,
            None => return,
        };
        let first_loc = first_arg.location();
        let first_arg_src = &src[first_loc.start_offset()..first_loc.end_offset()];
        if first_arg_src != exist_arg_src {
            return;
        }

        // Check `force:` kwarg. `explicit_not_force?` (force: false) — skip both.
        // Else if `force: true` among args — skip method offense but emit exist offense.
        let mut force_opt: Option<bool> = None;
        for a in args.arguments().iter() {
            if let Some(f) = find_force_kwarg(&a) {
                force_opt = Some(f);
                if !f {
                    return; // explicit_not_force → skip
                }
                break;
            }
        }

        // Determine if modifier form (postfix if/unless): end_loc_opt is None.
        let is_modifier = end_loc_opt.is_none();

        // ---- Offenses ----

        // Offense A: method (unless force_method_name or force: true)
        let force_method = is_force_method(&target_method) || force_opt == Some(true);
        let mut offs: Vec<Offense> = Vec::new();
        if !force_method {
            let repl_m = replacement_method(&target_method);
            let tmsg = format!(
                "Use atomic file operation method `FileUtils.{}`.",
                repl_m
            );
            let tloc = target_call.location();
            offs.push(self.ctx.offense_with_range(
                self.cop_name,
                &tmsg,
                Severity::Warning,
                tloc.start_offset(),
                tloc.end_offset(),
            ));
        }

        // Offense B: exist check removal — range = keyword begin..condition end.
        let cond_loc = cond_loc_saved;
        let emsg = format!(
            "Remove unnecessary existence check `{}.{}`.",
            exist_receiver, exist_method
        );
        let exist_off = self.ctx.offense_with_range(
            self.cop_name,
            &emsg,
            Severity::Warning,
            kw_loc.start_offset(),
            cond_loc.end_offset(),
        );

        // Correction: only if NOT elsif branch.
        let corr_off = if !is_elsif_branch {
            let mut edits: Vec<Edit> = Vec::new();
            // 1. remove keyword..cond range
            edits.push(Edit {
                start_offset: kw_loc.start_offset(),
                end_offset: cond_loc.end_offset(),
                replacement: String::new(),
            });
            // 2. replace method receiver name with "FileUtils" (unless already FileUtils)
            if !force_method && target_recv_name != "FileUtils" {
                // find receiver name location in source — for const path or const read,
                // we want last segment. `target_recv.location()` covers whole receiver
                // including leading `::` if cbase. The RuboCop code replaces
                // `node.child_nodes.first.loc.name` = the const node's name loc. For a
                // simple ConstantReadNode, location = name.
                let rl = target_recv.location();
                let rs = &src[rl.start_offset()..rl.end_offset()];
                // If prefix is `::`, name_start is after `::`; otherwise whole span.
                let name_start = if rs.starts_with("::") {
                    rl.start_offset() + 2
                } else {
                    rl.start_offset()
                };
                let name_end = rl.end_offset();
                edits.push(Edit {
                    start_offset: name_start,
                    end_offset: name_end,
                    replacement: "FileUtils".to_string(),
                });
            }
            // 3. replace method selector with replacement name
            if !force_method {
                if let Some(sel) = target_call.message_loc() {
                    let repl = replacement_method(&target_method);
                    edits.push(Edit {
                        start_offset: sel.start_offset(),
                        end_offset: sel.end_offset(),
                        replacement: repl.to_string(),
                    });
                }
            }
            // 4. if Dir.mkdir with 2 args and replacement mkdir_p → insert "mode: " before last arg
            if target_recv_name == "Dir"
                && replacement_method(&target_method) == "mkdir_p"
                && args.arguments().iter().count() == 2
            {
                let last = args.arguments().iter().last().unwrap();
                let ll = last.location();
                edits.push(Edit {
                    start_offset: ll.start_offset(),
                    end_offset: ll.start_offset(),
                    replacement: "mode: ".to_string(),
                });
            }
            // 5. Remove end keyword OR remove ` unless/if ` space in modifier form.
            if is_modifier {
                // remove from target_call end to keyword start
                let te = target_call.location().end_offset();
                edits.push(Edit {
                    start_offset: te,
                    end_offset: kw_loc.start_offset(),
                    replacement: String::new(),
                });
            } else if let Some(el) = end_loc_opt {
                edits.push(Edit {
                    start_offset: el.start_offset(),
                    end_offset: el.end_offset(),
                    replacement: String::new(),
                });
            }
            // Sort edits ascending by start; apply_corrections applies descending.
            exist_off.with_correction(Correction { edits })
        } else {
            exist_off
        };

        offs.push(corr_off);

        // RuboCop emits method offense first, then exist offense. Our tester compares
        // offenses sorted by (line, col). For block form: method @ line 2 col 2; exist @
        // line 1 col 0. Sorted order = exist, method. For postfix: method @ col 0; exist
        // @ col 22. Sorted = method, exist. We push in any order; sort handles the rest.
        self.offenses.extend(offs);
    }
}

impl Cop for NonAtomicFileOperation {
    fn name(&self) -> &'static str {
        "Lint/NonAtomicFileOperation"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V {
            ctx,
            offenses: vec![],
            cop_name: self.name(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Lint/NonAtomicFileOperation", |_cfg| Some(Box::new(NonAtomicFileOperation::new())));
