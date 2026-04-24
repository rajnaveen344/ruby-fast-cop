//! Style/ArgumentsForwarding
//!
//! Port of RuboCop's `Style/ArgumentsForwarding` (Ruby ≥ 2.7).
//! Detects `def foo(*args, &block); bar(*args, &block); end` → `def foo(...); bar(...); end`.
//! On Ruby ≥ 3.2 also flags individual anon forwarding (`*`, `**`, `&`).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{
    BlockParameterNode, CallNode, DefNode, KeywordRestParameterNode, Node, ParametersNode,
    RestParameterNode, SuperNode, Visit, YieldNode,
};
use std::collections::HashSet;

const NAME: &str = "Style/ArgumentsForwarding";
const FORWARDING_MSG: &str = "Use shorthand syntax `...` for arguments forwarding.";
const ARGS_MSG: &str = "Use anonymous positional arguments forwarding (`*`).";
const KWARGS_MSG: &str = "Use anonymous keyword arguments forwarding (`**`).";
const BLOCK_MSG: &str = "Use anonymous block arguments forwarding (`&`).";

#[derive(Debug, Clone)]
pub struct ArgumentsForwarding {
    use_anonymous: bool,
    allow_only_rest: bool,
    redundant_rest_names: Vec<String>,
    redundant_kwrest_names: Vec<String>,
    redundant_block_names: Vec<String>,
    block_forwarding_explicit: bool,
}

impl ArgumentsForwarding {
    pub fn new() -> Self {
        Self {
            use_anonymous: true,
            allow_only_rest: true,
            redundant_rest_names: vec!["args".into(), "arguments".into()],
            redundant_kwrest_names: vec!["kwargs".into(), "options".into(), "opts".into()],
            redundant_block_names: vec!["blk".into(), "block".into(), "proc".into()],
            block_forwarding_explicit: false,
        }
    }
}

impl Default for ArgumentsForwarding {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for ArgumentsForwarding {
    fn name(&self) -> &'static str {
        NAME
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 7) {
            return vec![];
        }
        let mut v = Visitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            seen: HashSet::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a ArgumentsForwarding,
    offenses: Vec<Offense>,
    seen: HashSet<(usize, usize, &'static str)>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_def_node(&mut self, node: &DefNode) {
        self.process_def(node);
        ruby_prism::visit_def_node(self, node);
    }
}

// ── Lite (owned/no-lifetime) data structures ──

#[derive(Debug, Default, Clone)]
struct ParamLite {
    /// Range of the param node (e.g. `*args`, `**kwargs`, `&block`).
    range: (usize, usize),
    /// Variable name without sigil (e.g. "args"), or None if anonymous.
    name: Option<String>,
    /// Source text including sigil (e.g. "*args" or "*").
    source: String,
}

#[derive(Debug, Default, Clone)]
struct ForwardableLite {
    rest: Option<ParamLite>,
    kwrest: Option<ParamLite>,
    block: Option<ParamLite>,
}

#[derive(Debug, Clone, Copy)]
enum SendArgKind {
    /// Splat (`*x`) with named expression
    NamedSplat,
    /// Anonymous splat (`*`) — no expression
    AnonSplat,
    /// AssocSplat (`**x`) inside a (Keyword)HashNode that has only THIS element
    NamedKwSplatSole,
    /// AssocSplat (`**x`) inside a hash with OTHER elements
    NamedKwSplatExtra,
    /// AssocSplat (`**`) anonymous, sole element of hash
    AnonKwSplatSole,
    /// BlockArgument with named expression
    NamedBlock,
    /// BlockArgument anonymous (`&`)
    AnonBlock,
    /// Plain hash (with no kwsplat)
    Hash,
    /// Other arg (literal, var, etc.)
    Other,
}

#[derive(Debug, Clone)]
struct SendArgLite {
    kind: SendArgKind,
    /// Whole range of the arg as it appears in send args (the splat including `*`,
    /// the assoc_splat including `**`, the whole hash, etc.). For kwsplat args
    /// inside a hash, this is the AssocSplatNode location, NOT the hash.
    range: (usize, usize),
    /// For named kinds, the variable name from the inner LocalVariableReadNode.
    name: Option<String>,
}

#[derive(Debug, Clone)]
struct SendLite {
    args: Vec<SendArgLite>,
    /// Range of the send/super/yield node
    range: (usize, usize),
    /// Where to insert paren `(` if missing — end of method name / keyword
    paren_open_at: usize,
    /// Start of first arg (used to replace [paren_open_at..first_arg_start] with "(" when paren-less)
    first_arg_start: usize,
    /// Whether the call already has parens
    has_parens: bool,
    /// Whether method is `[]` (skip paren-add for those)
    is_index_method: bool,
    /// End of last arg (for paren-close insertion AND forward-all replacement end)
    last_arg_end: usize,
    /// Whether any ancestor is a block (for Ruby 3.3 nesting check)
    in_block: bool,
}

#[derive(Debug, Clone, Copy)]
enum Classification {
    All,
    AllAnonymous,
    RestOrKwrest,
}

#[derive(Debug, Clone)]
struct SendClass {
    classification: Classification,
    fwd_rest: Option<(usize, usize)>,
    fwd_kwrest: Option<(usize, usize)>,
    fwd_block: Option<(usize, usize)>,
    paren_open_at: usize,
    first_arg_start: usize,
    has_parens: bool,
    is_index_method: bool,
    last_arg_end: usize,
    /// `forward_all_first_argument`: location of anonymous splat (`*`) in send args, used as start for `:all_anonymous` forward-all range.
    anon_first_arg_start: Option<usize>,
    in_block: bool,
}

impl<'a> Visitor<'a> {
    fn push(&mut self, start: usize, end: usize, msg: &'static str, edits: Vec<Edit>) {
        if !self.seen.insert((start, end, msg)) {
            return;
        }
        let mut o = self
            .ctx
            .offense_with_range(NAME, msg, Severity::Convention, start, end);
        if !edits.is_empty() {
            o = o.with_correction(Correction { edits });
        }
        self.offenses.push(o);
    }

    fn process_def(&mut self, def_node: &DefNode<'_>) {
        let Some(body) = def_node.body() else { return };
        let Some(params) = def_node.parameters() else { return };

        let forwardable = extract_forwardable(&params);
        if forwardable.rest.is_none() && forwardable.kwrest.is_none() && forwardable.block.is_none()
        {
            return;
        }

        let redundant = self.redundant_filter(forwardable);
        if redundant.rest.is_none() && redundant.kwrest.is_none() && redundant.block.is_none() {
            return;
        }

        let referenced = collect_referenced_lvars(&body);

        let sends = collect_sends(&body);
        if sends.is_empty() {
            return;
        }

        let classifications: Vec<SendClass> = sends
            .into_iter()
            .filter_map(|s| self.classify(&params, &redundant, &referenced, s))
            .collect();

        if classifications.is_empty() {
            return;
        }

        let only_all = classifications
            .iter()
            .all(|c| matches!(c.classification, Classification::All | Classification::AllAnonymous));

        // Compute def-side context for emitting
        let def_has_parens = def_node.lparen_loc().is_some();
        let params_range = (
            params.location().start_offset(),
            params.location().end_offset(),
        );
        let def_name_end = def_node.name_loc().end_offset();
        let def_last_arg_end = redundant
            .block
            .as_ref()
            .map(|b| b.range.1)
            .unwrap_or(params_range.1);

        if only_all {
            self.add_forward_all_offenses(
                &redundant,
                &classifications,
                def_has_parens,
                params_range,
                def_name_end,
                def_last_arg_end,
            );
        } else if self.ctx.ruby_version_at_least(3, 2) {
            self.add_post_ruby_32_offenses(
                &redundant,
                &classifications,
                def_has_parens,
                params_range,
                def_name_end,
                params_extra(&params),
            );
        }
    }

    fn redundant_filter(&self, fa: ForwardableLite) -> ForwardableLite {
        let rest = fa
            .rest
            .filter(|r| is_redundant(&r.source, "*", &self.cop.redundant_rest_names));
        let kwrest = fa
            .kwrest
            .filter(|r| is_redundant(&r.source, "**", &self.cop.redundant_kwrest_names));
        let block = fa
            .block
            .filter(|r| is_redundant(&r.source, "&", &self.cop.redundant_block_names));
        ForwardableLite { rest, kwrest, block }
    }

    fn classify(
        &self,
        params: &ParametersNode<'_>,
        redundant: &ForwardableLite,
        referenced: &HashSet<String>,
        send: SendLite,
    ) -> Option<SendClass> {
        let rest_name = redundant.rest.as_ref().and_then(|p| p.name.clone());
        let kwrest_name = redundant.kwrest.as_ref().and_then(|p| p.name.clone());
        let block_name = redundant.block.as_ref().and_then(|p| p.name.clone());

        let referenced_rest = rest_name.as_ref().is_some_and(|n| referenced.contains(n));
        let referenced_kwrest = kwrest_name.as_ref().is_some_and(|n| referenced.contains(n));
        let referenced_block = block_name.as_ref().is_some_and(|n| referenced.contains(n));

        // Find forwarded args within send.args
        let fwd_rest = if !referenced_rest {
            send.args.iter().find_map(|a| match (a.kind, a.name.as_deref(), rest_name.as_deref()) {
                (SendArgKind::NamedSplat, Some(n), Some(rn)) if n == rn => Some(a.range),
                _ => None,
            })
        } else {
            None
        };
        let mut fwd_kwrest_extra = false;
        let fwd_kwrest = if !referenced_kwrest {
            send.args.iter().find_map(|a| match (a.kind, a.name.as_deref(), kwrest_name.as_deref()) {
                (SendArgKind::NamedKwSplatSole, Some(n), Some(kn)) if n == kn => Some(a.range),
                (SendArgKind::NamedKwSplatExtra, Some(n), Some(kn)) if n == kn => {
                    fwd_kwrest_extra = true;
                    Some(a.range)
                }
                _ => None,
            })
        } else {
            None
        };
        let fwd_block = if !referenced_block {
            send.args.iter().find_map(|a| match (a.kind, a.name.as_deref(), block_name.as_deref()) {
                (SendArgKind::NamedBlock, Some(n), Some(bn)) if n == bn => Some(a.range),
                (SendArgKind::AnonBlock, _, _) => Some(a.range),
                _ => None,
            })
        } else {
            None
        };

        if fwd_rest.is_none() && fwd_kwrest.is_none() && fwd_block.is_none() {
            return None;
        }

        let target_ge_30 = self.ctx.ruby_version_at_least(3, 0);
        let target_ge_32 = self.ctx.ruby_version_at_least(3, 2);

        let def_all_anon = is_def_all_anonymous(params);
        let send_all_anon = is_send_all_anonymous(&send.args);
        let any_arg_referenced = referenced_rest || referenced_kwrest || referenced_block;

        let ruby_32_only_anonymous_forwarding =
            !send.in_block && def_all_anon && send_all_anon;

        let any_optarg = params.optionals().iter().count() > 0;
        let target_ge_31 = self.ctx.ruby_version_at_least(3, 1);
        let ruby_30_or_lower_optarg = !target_ge_31 && any_optarg;

        let ruby_32_or_higher_missing =
            target_ge_32 && !(fwd_rest.is_some() && fwd_kwrest.is_some());

        let offensive_block_forwarding = if redundant.block.is_some() {
            fwd_block.is_some()
        } else {
            !self.cop.allow_only_rest
        };

        let additional_kwargs = has_kw_or_kwopt(params);
        let additional_kwargs_or_forwarded = additional_kwargs || fwd_kwrest_extra;

        let forwardable_count = redundant.rest.is_some() as usize
            + redundant.kwrest.is_some() as usize
            + redundant.block.is_some() as usize;
        let missing_rest_or_kwrest = (rest_name.is_some() && fwd_rest.is_none())
            || (kwrest_name.is_some() && fwd_kwrest.is_none());
        let def_arg_count = total_param_count(params);
        let no_additional_args = !missing_rest_or_kwrest
            && def_arg_count == forwardable_count
            && send.args.len() == forwardable_count;

        let no_post_splat_args = match fwd_rest {
            None => true,
            Some(rest_range) => {
                let idx = send.args.iter().position(|a| a.range == rest_range);
                match idx {
                    None => true,
                    Some(i) => match send.args.get(i + 1) {
                        None => true,
                        Some(after) => matches!(
                            after.kind,
                            SendArgKind::Hash
                                | SendArgKind::NamedKwSplatSole
                                | SendArgKind::NamedKwSplatExtra
                                | SendArgKind::AnonKwSplatSole
                                | SendArgKind::NamedBlock
                                | SendArgKind::AnonBlock
                        ),
                    },
                }
            }
        };

        let can_forward_all = !any_arg_referenced
            && !ruby_30_or_lower_optarg
            && !ruby_32_or_higher_missing
            && offensive_block_forwarding
            && !additional_kwargs_or_forwarded
            && (no_additional_args || (target_ge_30 && no_post_splat_args));

        let classification = if ruby_32_only_anonymous_forwarding {
            Classification::AllAnonymous
        } else if can_forward_all {
            Classification::All
        } else {
            Classification::RestOrKwrest
        };

        let anon_first_arg_start = if matches!(classification, Classification::AllAnonymous) {
            send.args
                .iter()
                .rev()
                .find_map(|a| match a.kind {
                    SendArgKind::AnonSplat => Some(a.range.0),
                    _ => None,
                })
        } else {
            None
        };

        Some(SendClass {
            classification,
            fwd_rest,
            fwd_kwrest,
            fwd_block,
            paren_open_at: send.paren_open_at,
            first_arg_start: send.first_arg_start,
            has_parens: send.has_parens,
            is_index_method: send.is_index_method,
            last_arg_end: send.last_arg_end,
            anon_first_arg_start,
            in_block: send.in_block,
        })
    }

    fn add_forward_all_offenses(
        &mut self,
        redundant: &ForwardableLite,
        classifications: &[SendClass],
        def_has_parens: bool,
        params_range: (usize, usize),
        def_name_end: usize,
        def_last_arg_end: usize,
    ) {
        let mut registered_block_arg_offense = false;
        let target_ge_34 = self.ctx.ruby_version_at_least(3, 4);

        for c in classifications {
            // "only block forwarded" branch
            if c.fwd_rest.is_none()
                && c.fwd_kwrest.is_none()
                && !matches!(c.classification, Classification::AllAnonymous)
            {
                if let Some(block_range) = c.fwd_block {
                    let allow_in_block = target_ge_34 || !c.in_block;
                    if allow_in_block {
                        if let Some(bp) = redundant.block.as_ref() {
                            self.emit_block_arg_def(
                                bp,
                                def_has_parens,
                                params_range,
                                def_name_end,
                                true,
                            );
                        }
                        self.emit_block_arg_send(c, block_range, true);
                    }
                }
                registered_block_arg_offense = true;
                break;
            }

            let first_start = c
                .fwd_rest
                .map(|r| r.0)
                .or(c.fwd_kwrest.map(|r| r.0))
                .or(c.anon_first_arg_start)
                .unwrap_or(c.paren_open_at);
            let last_end = c.last_arg_end;
            self.emit_forward_all_send(c, first_start, last_end);
        }

        if registered_block_arg_offense {
            return;
        }

        // Def-side forward-all
        let first_start = redundant
            .rest
            .as_ref()
            .map(|r| r.range.0)
            .or(redundant.kwrest.as_ref().map(|r| r.range.0))
            .or(redundant.block.as_ref().map(|r| r.range.0));
        if let Some(s) = first_start {
            self.emit_forward_all_def(s, def_last_arg_end, def_has_parens, params_range, def_name_end);
        }
    }

    fn add_post_ruby_32_offenses(
        &mut self,
        redundant: &ForwardableLite,
        classifications: &[SendClass],
        def_has_parens: bool,
        params_range: (usize, usize),
        def_name_end: usize,
        _params_info: ParamsInfo,
    ) {
        if !self.cop.use_anonymous {
            return;
        }
        let target_ge_34 = self.ctx.ruby_version_at_least(3, 4);
        let all_correctable = target_ge_34 || classifications.iter().all(|c| !c.in_block);
        if !all_correctable {
            return;
        }

        for c in classifications {
            let allow_anon = |in_block: bool| -> bool { target_ge_34 || !in_block };

            if let Some(rest_range) = c.fwd_rest {
                if allow_anon(c.in_block) {
                    if let Some(rp) = redundant.rest.as_ref() {
                        self.emit_anon_def_replace(
                            rp,
                            "*",
                            ARGS_MSG,
                            true,
                            def_has_parens,
                            params_range,
                            def_name_end,
                        );
                    }
                    self.emit_anon_send_replace(c, rest_range, "*", ARGS_MSG, true);
                }
            }
            if let Some(kw_range) = c.fwd_kwrest {
                if allow_anon(c.in_block) {
                    let add_parens = c.fwd_rest.is_none();
                    if let Some(kp) = redundant.kwrest.as_ref() {
                        self.emit_anon_def_replace(
                            kp,
                            "**",
                            KWARGS_MSG,
                            add_parens,
                            def_has_parens,
                            params_range,
                            def_name_end,
                        );
                    }
                    self.emit_anon_send_replace(c, kw_range, "**", KWARGS_MSG, add_parens);
                }
            }
            if let Some(blk_range) = c.fwd_block {
                if allow_anon(c.in_block) {
                    let add_parens = c.fwd_rest.is_none();
                    if let Some(bp) = redundant.block.as_ref() {
                        self.emit_block_arg_def(bp, def_has_parens, params_range, def_name_end, add_parens);
                    }
                    self.emit_block_arg_send(c, blk_range, add_parens);
                }
            }
        }
    }

    // ── Emitters ──

    fn emit_forward_all_def(
        &mut self,
        start: usize,
        end: usize,
        def_has_parens: bool,
        params_range: (usize, usize),
        def_name_end: usize,
    ) {
        let mut edits = Vec::new();
        if !def_has_parens {
            // Replace [name_end..first_param_start] with "(", eating any whitespace
            edits.push(Edit {
                start_offset: def_name_end,
                end_offset: params_range.0,
                replacement: "(".into(),
            });
            edits.push(Edit {
                start_offset: params_range.1,
                end_offset: params_range.1,
                replacement: ")".into(),
            });
        }
        edits.push(Edit {
            start_offset: start,
            end_offset: end,
            replacement: "...".into(),
        });
        self.push(start, end, FORWARDING_MSG, edits);
    }

    fn emit_forward_all_send(&mut self, c: &SendClass, start: usize, end: usize) {
        let mut edits = Vec::new();
        if !c.has_parens && !c.is_index_method {
            push_paren_edits(c, &mut edits);
        }
        edits.push(Edit {
            start_offset: start,
            end_offset: end,
            replacement: "...".into(),
        });
        self.push(start, end, FORWARDING_MSG, edits);
    }

    fn emit_anon_def_replace(
        &mut self,
        p: &ParamLite,
        repl: &str,
        msg: &'static str,
        add_parens: bool,
        def_has_parens: bool,
        params_range: (usize, usize),
        def_name_end: usize,
    ) {
        let (s, e) = p.range;
        let mut edits = Vec::new();
        if add_parens && !def_has_parens {
            edits.push(Edit {
                start_offset: def_name_end,
                end_offset: params_range.0,
                replacement: "(".into(),
            });
            edits.push(Edit {
                start_offset: params_range.1,
                end_offset: params_range.1,
                replacement: ")".into(),
            });
        }
        edits.push(Edit {
            start_offset: s,
            end_offset: e,
            replacement: repl.into(),
        });
        self.push(s, e, msg, edits);
    }

    fn emit_block_arg_def(
        &mut self,
        bp: &ParamLite,
        def_has_parens: bool,
        params_range: (usize, usize),
        def_name_end: usize,
        add_parens: bool,
    ) {
        if !self.ctx.ruby_version_at_least(3, 1) {
            return;
        }
        let (s, e) = bp.range;
        if e - s == 1 {
            return; // already '&'
        }
        if self.cop.block_forwarding_explicit {
            return;
        }
        self.emit_anon_def_replace(bp, "&", BLOCK_MSG, add_parens, def_has_parens, params_range, def_name_end);
    }

    fn emit_block_arg_send(&mut self, c: &SendClass, range: (usize, usize), add_parens: bool) {
        if !self.ctx.ruby_version_at_least(3, 1) {
            return;
        }
        let (s, e) = range;
        if e - s == 1 {
            return;
        }
        if self.cop.block_forwarding_explicit {
            return;
        }
        let mut edits = Vec::new();
        if add_parens && !c.has_parens && !c.is_index_method {
            push_paren_edits(c, &mut edits);
        }
        edits.push(Edit {
            start_offset: s,
            end_offset: e,
            replacement: "&".into(),
        });
        self.push(s, e, BLOCK_MSG, edits);
    }

    fn emit_anon_send_replace(
        &mut self,
        c: &SendClass,
        range: (usize, usize),
        repl: &str,
        msg: &'static str,
        add_parens: bool,
    ) {
        let (s, e) = range;
        let mut edits = Vec::new();
        if add_parens && !c.has_parens && !c.is_index_method {
            push_paren_edits(c, &mut edits);
        }
        edits.push(Edit {
            start_offset: s,
            end_offset: e,
            replacement: repl.into(),
        });
        self.push(s, e, msg, edits);
    }
}

fn push_paren_edits(c: &SendClass, edits: &mut Vec<Edit>) {
    edits.push(Edit {
        start_offset: c.paren_open_at,
        end_offset: c.first_arg_start,
        replacement: "(".into(),
    });
    edits.push(Edit {
        start_offset: c.last_arg_end,
        end_offset: c.last_arg_end,
        replacement: ")".into(),
    });
}

// ── Owned param info we may want later ──

#[derive(Default)]
struct ParamsInfo;

fn params_extra(_params: &ParametersNode<'_>) -> ParamsInfo {
    ParamsInfo
}

// ── Static helpers ──

fn is_redundant(source: &str, sigil: &str, names: &[String]) -> bool {
    if source == sigil {
        return true;
    }
    for n in names {
        if source == format!("{sigil}{n}") {
            return true;
        }
    }
    false
}

fn extract_forwardable(params: &ParametersNode<'_>) -> ForwardableLite {
    let rest = params
        .rest()
        .and_then(|n| n.as_rest_parameter_node())
        .map(|r| ParamLite {
            range: (r.location().start_offset(), r.location().end_offset()),
            name: r
                .name_loc()
                .map(|l| String::from_utf8_lossy(l.as_slice()).into_owned()),
            source: String::from_utf8_lossy(r.location().as_slice()).into_owned(),
        });
    let kwrest = params
        .keyword_rest()
        .and_then(|n| n.as_keyword_rest_parameter_node())
        .map(|r| ParamLite {
            range: (r.location().start_offset(), r.location().end_offset()),
            name: r
                .name_loc()
                .map(|l| String::from_utf8_lossy(l.as_slice()).into_owned()),
            source: String::from_utf8_lossy(r.location().as_slice()).into_owned(),
        });
    let block = params.block().map(|r| ParamLite {
        range: (r.location().start_offset(), r.location().end_offset()),
        name: r
            .name_loc()
            .map(|l| String::from_utf8_lossy(l.as_slice()).into_owned()),
        source: String::from_utf8_lossy(r.location().as_slice()).into_owned(),
    });
    ForwardableLite { rest, kwrest, block }
}

fn total_param_count(params: &ParametersNode) -> usize {
    let mut n = 0;
    n += params.requireds().iter().count();
    n += params.optionals().iter().count();
    n += params.rest().is_some() as usize;
    n += params.posts().iter().count();
    n += params.keywords().iter().count();
    n += params.keyword_rest().is_some() as usize;
    n += params.block().is_some() as usize;
    n
}

fn has_kw_or_kwopt(params: &ParametersNode) -> bool {
    params.keywords().iter().count() > 0
}

fn is_def_all_anonymous(params: &ParametersNode) -> bool {
    let rest_anon = params
        .rest()
        .and_then(|n| n.as_rest_parameter_node())
        .is_some_and(|r| r.name_loc().is_none());
    let kwrest_anon = params
        .keyword_rest()
        .and_then(|n| n.as_keyword_rest_parameter_node())
        .is_some_and(|r| r.name_loc().is_none());
    let block_anon = params.block().is_some_and(|b| b.name_loc().is_none());
    rest_anon && kwrest_anon && block_anon
}

fn is_send_all_anonymous(args: &[SendArgLite]) -> bool {
    let mut has_anon_splat = false;
    let mut has_anon_kw = false;
    let mut has_anon_block = false;
    for a in args {
        match a.kind {
            SendArgKind::AnonSplat => has_anon_splat = true,
            SendArgKind::AnonKwSplatSole => has_anon_kw = true,
            SendArgKind::AnonBlock => has_anon_block = true,
            _ => {}
        }
    }
    has_anon_splat && has_anon_kw && has_anon_block
}

fn collect_referenced_lvars(body: &Node) -> HashSet<String> {
    struct V {
        names: HashSet<String>,
        in_forwarding_arg: usize,
    }
    impl<'a> Visit<'_> for V {
        fn visit_splat_node(&mut self, node: &ruby_prism::SplatNode) {
            self.in_forwarding_arg += 1;
            ruby_prism::visit_splat_node(self, node);
            self.in_forwarding_arg -= 1;
        }
        fn visit_assoc_splat_node(&mut self, node: &ruby_prism::AssocSplatNode) {
            self.in_forwarding_arg += 1;
            ruby_prism::visit_assoc_splat_node(self, node);
            self.in_forwarding_arg -= 1;
        }
        fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode) {
            self.in_forwarding_arg += 1;
            ruby_prism::visit_block_argument_node(self, node);
            self.in_forwarding_arg -= 1;
        }
        fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
            if self.in_forwarding_arg > 0 {
                return;
            }
            self.names
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
        }
        fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
            self.names
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            ruby_prism::visit_local_variable_write_node(self, node);
        }
        fn visit_local_variable_and_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableAndWriteNode,
        ) {
            self.names
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            ruby_prism::visit_local_variable_and_write_node(self, node);
        }
        fn visit_local_variable_or_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableOrWriteNode,
        ) {
            self.names
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            ruby_prism::visit_local_variable_or_write_node(self, node);
        }
        fn visit_local_variable_operator_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableOperatorWriteNode,
        ) {
            self.names
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            ruby_prism::visit_local_variable_operator_write_node(self, node);
        }
    }
    let mut v = V {
        names: HashSet::new(),
        in_forwarding_arg: 0,
    };
    v.visit(body);
    v.names
}

fn collect_sends(body: &Node) -> Vec<SendLite> {
    struct V {
        in_block: usize,
        sites: Vec<SendLite>,
    }
    impl<'a> Visit<'_> for V {
        fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
            self.in_block += 1;
            ruby_prism::visit_block_node(self, node);
            self.in_block -= 1;
        }
        fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
            self.in_block += 1;
            ruby_prism::visit_lambda_node(self, node);
            self.in_block -= 1;
        }
        fn visit_def_node(&mut self, _node: &DefNode) {}
        fn visit_call_node(&mut self, node: &CallNode) {
            self.collect_call(node);
            ruby_prism::visit_call_node(self, node);
        }
        fn visit_super_node(&mut self, node: &SuperNode) {
            self.collect_super(node);
            ruby_prism::visit_super_node(self, node);
        }
        fn visit_yield_node(&mut self, node: &YieldNode) {
            self.collect_yield(node);
            ruby_prism::visit_yield_node(self, node);
        }
    }
    impl V {
        fn collect_call(&mut self, node: &CallNode) {
            let args_node = node.arguments();
            let mut raw_args: Vec<Node> = args_node
                .as_ref()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            // CallNode.block() returns the &block-style BlockArgumentNode (or
            // a BlockNode for do/end). Only the former counts as a send arg
            // for forwarding analysis.
            if let Some(blk) = node.block() {
                if blk.as_block_argument_node().is_some() {
                    raw_args.push(blk);
                }
            }
            if raw_args
                .iter()
                .any(|a| a.as_forwarding_arguments_node().is_some())
            {
                return;
            }
            let args = materialize_args(&raw_args);
            let opening = node.opening_loc();
            let has_parens = opening.is_some();
            let method_end = node
                .message_loc()
                .map(|l| l.end_offset())
                .unwrap_or_else(|| node.location().end_offset());
            let paren_open_at = method_end;
            let last_arg_end = raw_args
                .last()
                .map(|a| a.location().end_offset())
                .unwrap_or(method_end);
            let first_arg_start = raw_args
                .first()
                .map(|a| a.location().start_offset())
                .unwrap_or(method_end);
            let is_index = node.name().as_slice() == b"[]";
            let l = node.location();
            self.sites.push(SendLite {
                args,
                range: (l.start_offset(), l.end_offset()),
                paren_open_at,
                first_arg_start,
                has_parens,
                is_index_method: is_index,
                last_arg_end,
                in_block: self.in_block > 0,
            });
        }

        fn collect_super(&mut self, node: &SuperNode) {
            let args_node = node.arguments();
            let mut raw_args: Vec<Node> = args_node
                .as_ref()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if let Some(blk) = node.block() {
                if blk.as_block_argument_node().is_some() {
                    raw_args.push(blk);
                }
            }
            if raw_args
                .iter()
                .any(|a| a.as_forwarding_arguments_node().is_some())
            {
                return;
            }
            let args = materialize_args(&raw_args);
            let kw_end = node.keyword_loc().end_offset();
            let has_parens = node.lparen_loc().is_some();
            let last_arg_end = raw_args
                .last()
                .map(|a| a.location().end_offset())
                .unwrap_or(kw_end);
            let first_arg_start = raw_args
                .first()
                .map(|a| a.location().start_offset())
                .unwrap_or(kw_end);
            let l = node.location();
            self.sites.push(SendLite {
                args,
                range: (l.start_offset(), l.end_offset()),
                paren_open_at: kw_end,
                first_arg_start,
                has_parens,
                is_index_method: false,
                last_arg_end,
                in_block: self.in_block > 0,
            });
        }

        fn collect_yield(&mut self, node: &YieldNode) {
            let args_node = node.arguments();
            let raw_args: Vec<Node> = args_node
                .as_ref()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if raw_args
                .iter()
                .any(|a| a.as_forwarding_arguments_node().is_some())
            {
                return;
            }
            let args = materialize_args(&raw_args);
            let kw_end = node.keyword_loc().end_offset();
            let has_parens = node.lparen_loc().is_some();
            let last_arg_end = raw_args
                .last()
                .map(|a| a.location().end_offset())
                .unwrap_or(kw_end);
            let first_arg_start = raw_args
                .first()
                .map(|a| a.location().start_offset())
                .unwrap_or(kw_end);
            let l = node.location();
            self.sites.push(SendLite {
                args,
                range: (l.start_offset(), l.end_offset()),
                paren_open_at: kw_end,
                first_arg_start,
                has_parens,
                is_index_method: false,
                last_arg_end,
                in_block: self.in_block > 0,
            });
        }
    }
    let mut v = V {
        in_block: 0,
        sites: Vec::new(),
    };
    v.visit(body);
    v.sites
}

fn materialize_args(raw_args: &[Node]) -> Vec<SendArgLite> {
    let mut out = Vec::with_capacity(raw_args.len());
    for a in raw_args {
        let l = a.location();
        let range = (l.start_offset(), l.end_offset());
        if let Some(sp) = a.as_splat_node() {
            match sp.expression() {
                None => out.push(SendArgLite { kind: SendArgKind::AnonSplat, range, name: None }),
                Some(expr) => {
                    let name = expr
                        .as_local_variable_read_node()
                        .map(|lvr| String::from_utf8_lossy(lvr.name().as_slice()).into_owned());
                    if name.is_some() {
                        out.push(SendArgLite { kind: SendArgKind::NamedSplat, range, name });
                    } else {
                        out.push(SendArgLite { kind: SendArgKind::Other, range, name: None });
                    }
                }
            }
        } else if let Some(ba) = a.as_block_argument_node() {
            match ba.expression() {
                None => out.push(SendArgLite { kind: SendArgKind::AnonBlock, range, name: None }),
                Some(expr) => {
                    let name = expr
                        .as_local_variable_read_node()
                        .map(|lvr| String::from_utf8_lossy(lvr.name().as_slice()).into_owned());
                    if name.is_some() {
                        out.push(SendArgLite { kind: SendArgKind::NamedBlock, range, name });
                    } else {
                        out.push(SendArgLite { kind: SendArgKind::Other, range, name: None });
                    }
                }
            }
        } else if let Some(kh) = a.as_keyword_hash_node() {
            // Look for AssocSplat inside; classify based on (sole vs extra) and named vs anon
            let elems: Vec<Node> = kh.elements().iter().collect();
            let kwsplat = elems.iter().enumerate().find_map(|(i, e)| {
                e.as_assoc_splat_node().map(|s| (i, s))
            });
            match kwsplat {
                Some((_, asp)) => {
                    let asp_loc = asp.location();
                    let asp_range = (asp_loc.start_offset(), asp_loc.end_offset());
                    match asp.value() {
                        None => out.push(SendArgLite {
                            kind: SendArgKind::AnonKwSplatSole,
                            range: asp_range,
                            name: None,
                        }),
                        Some(expr) => {
                            let name = expr
                                .as_local_variable_read_node()
                                .map(|lvr| {
                                    String::from_utf8_lossy(lvr.name().as_slice()).into_owned()
                                });
                            if elems.len() == 1 {
                                out.push(SendArgLite {
                                    kind: SendArgKind::NamedKwSplatSole,
                                    range: asp_range,
                                    name,
                                });
                            } else {
                                out.push(SendArgLite {
                                    kind: SendArgKind::NamedKwSplatExtra,
                                    range: asp_range,
                                    name,
                                });
                            }
                        }
                    }
                }
                None => out.push(SendArgLite { kind: SendArgKind::Hash, range, name: None }),
            }
        } else if a.as_hash_node().is_some() {
            // Explicit hash (like `{**x}`) — treat similarly to keyword_hash
            let hn = a.as_hash_node().unwrap();
            let elems: Vec<Node> = hn.elements().iter().collect();
            let kwsplat = elems.iter().enumerate().find_map(|(i, e)| {
                e.as_assoc_splat_node().map(|s| (i, s))
            });
            match kwsplat {
                Some((_, asp)) => {
                    let asp_loc = asp.location();
                    let asp_range = (asp_loc.start_offset(), asp_loc.end_offset());
                    match asp.value() {
                        None => out.push(SendArgLite {
                            kind: SendArgKind::AnonKwSplatSole,
                            range: asp_range,
                            name: None,
                        }),
                        Some(expr) => {
                            let name = expr
                                .as_local_variable_read_node()
                                .map(|lvr| {
                                    String::from_utf8_lossy(lvr.name().as_slice()).into_owned()
                                });
                            if elems.len() == 1 {
                                out.push(SendArgLite {
                                    kind: SendArgKind::NamedKwSplatSole,
                                    range: asp_range,
                                    name,
                                });
                            } else {
                                out.push(SendArgLite {
                                    kind: SendArgKind::NamedKwSplatExtra,
                                    range: asp_range,
                                    name,
                                });
                            }
                        }
                    }
                }
                None => out.push(SendArgLite { kind: SendArgKind::Hash, range, name: None }),
            }
        } else {
            out.push(SendArgLite { kind: SendArgKind::Other, range, name: None });
        }
    }
    out
}

fn parse_str_list(value: &serde_yaml::Value) -> Option<Vec<String>> {
    if value.is_null() {
        return None;
    }
    let seq = value.as_sequence()?;
    Some(
        seq.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
    )
}

crate::register_cop!("Style/ArgumentsForwarding", |cfg| {
    let cop_cfg = cfg.get_cop_config("Style/ArgumentsForwarding");
    let mut c = ArgumentsForwarding::new();
    if let Some(entry) = cop_cfg {
        if let Some(v) = entry.raw.get("UseAnonymousForwarding").and_then(|v| v.as_bool()) {
            c.use_anonymous = v;
        }
        if let Some(v) = entry.raw.get("AllowOnlyRestArgument").and_then(|v| v.as_bool()) {
            c.allow_only_rest = v;
        }
        if let Some(list) = entry.raw.get("RedundantRestArgumentNames").and_then(parse_str_list) {
            c.redundant_rest_names = list;
        }
        if let Some(list) = entry.raw.get("RedundantKeywordRestArgumentNames").and_then(parse_str_list) {
            c.redundant_kwrest_names = list;
        }
        if let Some(list) = entry.raw.get("RedundantBlockArgumentNames").and_then(parse_str_list) {
            c.redundant_block_names = list;
        }
    }
    let bf = cfg.get_cop_config("Naming/BlockForwarding");
    if let Some(entry) = bf {
        if let Some(s) = entry.enforced_style.as_deref() {
            if s == "explicit" {
                c.block_forwarding_explicit = true;
            }
        }
    }
    Some(Box::new(c))
});
