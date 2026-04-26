//! Bundler/GemComment - require explanatory comment on gem declarations.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/bundler/gem_comment.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

const MSG: &str = "Missing gem description comment.";
const VERSION_SPECIFIERS: &str = "version_specifiers";
const RESTRICTIVE_VERSION_SPECIFIERS: &str = "restrictive_version_specifiers";

pub struct GemComment {
    ignored_gems: Vec<String>,
    only_for: Vec<String>,
}

impl GemComment {
    pub fn new(ignored_gems: Vec<String>, only_for: Vec<String>) -> Self {
        Self { ignored_gems, only_for }
    }
}

impl Default for GemComment {
    fn default() -> Self { Self::new(Vec::new(), Vec::new()) }
}

fn is_gemfile(filename: &str) -> bool {
    let basename = filename.rsplit('/').next().unwrap_or(filename);
    basename == "Gemfile" || basename == "gems.rb" || basename.ends_with(".gemfile")
}

fn first_string_arg(call: &ruby_prism::CallNode) -> Option<String> {
    let args = call.arguments()?;
    let first = args.arguments().iter().next()?;
    let s = first.as_string_node()?;
    Some(String::from_utf8_lossy(s.unescaped()).to_string())
}

/// /\A\s*(?:<|~>|\d|=)/
fn restrictive_version(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    if i >= bytes.len() { return false; }
    let b = bytes[i];
    if b == b'<' || b == b'=' || b.is_ascii_digit() {
        return true;
    }
    if b == b'~' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
        return true;
    }
    false
}

/// Collect string version-specifier args (positional, after gem name).
fn version_str_args(call: &ruby_prism::CallNode) -> Vec<String> {
    let mut out = Vec::new();
    let Some(args) = call.arguments() else { return out; };
    for (i, arg) in args.arguments().iter().enumerate() {
        if i == 0 { continue; }
        if let Some(s) = arg.as_string_node() {
            out.push(String::from_utf8_lossy(s.unescaped()).to_string());
        }
    }
    out
}

fn version_specified(call: &ruby_prism::CallNode) -> bool {
    !version_str_args(call).is_empty()
}

fn restrictive_version_specified(call: &ruby_prism::CallNode) -> bool {
    version_str_args(call).iter().any(|s| restrictive_version(s))
}

/// Returns symbol/string keys from the trailing hash argument.
fn gem_options(call: &ruby_prism::CallNode) -> Vec<String> {
    let Some(args) = call.arguments() else { return vec![]; };
    let arg_list: Vec<_> = args.arguments().iter().collect();
    let Some(last) = arg_list.last() else { return vec![]; };
    let elements = if let Some(h) = last.as_keyword_hash_node() {
        h.elements()
    } else if let Some(h) = last.as_hash_node() {
        h.elements()
    } else {
        return vec![];
    };
    let mut keys = Vec::new();
    for el in elements.iter() {
        let Some(pair) = el.as_assoc_node() else { continue; };
        let key = pair.key();
        if let Some(sym) = key.as_symbol_node() {
            keys.push(String::from_utf8_lossy(sym.unescaped()).to_string());
        } else if let Some(s) = key.as_string_node() {
            keys.push(String::from_utf8_lossy(s.unescaped()).to_string());
        }
    }
    keys
}

impl GemComment {
    fn checked_options_present(&self, call: &ruby_prism::CallNode) -> bool {
        if self.only_for.iter().any(|s| s == VERSION_SPECIFIERS) && version_specified(call) {
            return true;
        }
        if self.only_for.iter().any(|s| s == RESTRICTIVE_VERSION_SPECIFIERS)
            && restrictive_version_specified(call)
        {
            return true;
        }
        let opts = gem_options(call);
        if self.only_for.iter().any(|o| opts.contains(o)) {
            return true;
        }
        false
    }
}

impl Cop for GemComment {
    fn name(&self) -> &'static str { "Bundler/GemComment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !is_gemfile(ctx.filename) { return vec![]; }
        let mut offenses = Vec::new();

        // Collect all comment lines.
        let parsed = ruby_prism::parse(ctx.source.as_bytes());
        let mut comment_lines: std::collections::BTreeSet<usize> = Default::default();
        for c in parsed.comments() {
            let loc = c.location();
            let line = ctx.line_of(loc.start_offset());
            comment_lines.insert(line);
        }

        // Walk top-level + group blocks for gem calls.
        let stmts = node.statements();
        for child in stmts.body().iter() {
            self.visit_node(&child, ctx, &comment_lines, &mut offenses);
        }
        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

impl GemComment {
    fn visit_node(
        &self,
        node: &ruby_prism::Node,
        ctx: &CheckContext,
        comment_lines: &std::collections::BTreeSet<usize>,
        offenses: &mut Vec<Offense>,
    ) {
        if let Some(call) = node.as_call_node() {
            if call.receiver().is_none() && node_name!(call) == "gem" {
                self.check_gem(&call, ctx, comment_lines, offenses);
                return;
            }
            // walk into block argument (e.g. `group :foo do ... end`)
            if let Some(block) = call.block() {
                if let Some(b) = block.as_block_node() {
                    if let Some(body) = b.body() {
                        if let Some(stmts) = body.as_statements_node() {
                            for child in stmts.body().iter() {
                                self.visit_node(&child, ctx, comment_lines, offenses);
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_gem(
        &self,
        call: &ruby_prism::CallNode,
        ctx: &CheckContext,
        comment_lines: &std::collections::BTreeSet<usize>,
        offenses: &mut Vec<Offense>,
    ) {
        let Some(name) = first_string_arg(call) else { return; };
        if self.ignored_gems.iter().any(|g| g == &name) { return; }

        let loc = call.location();
        let start_line = ctx.line_of(loc.start_offset());
        let end_line = ctx.line_of(loc.end_offset().saturating_sub(1));

        // commented if any comment on [start_line - 1 ..= end_line]
        let lo = start_line.saturating_sub(1).max(1);
        let commented = (lo..=end_line).any(|l| comment_lines.contains(&l));
        if commented { return; }

        if !self.only_for.is_empty() && !self.checked_options_present(call) { return; }

        offenses.push(ctx.offense_with_range(
            self.name(), MSG, self.severity(),
            loc.start_offset(), loc.end_offset(),
        ));
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct GemCommentCfg {
    ignored_gems: Vec<String>,
    only_for: Vec<String>,
}

crate::register_cop!("Bundler/GemComment", |cfg| {
    let c: GemCommentCfg = cfg.typed("Bundler/GemComment");
    Some(Box::new(GemComment::new(c.ignored_gems, c.only_for)))
});
