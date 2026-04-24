//! Security/IoMethods cop
//!
//! `IO.read`/`binread`/`write`/`binwrite`/`foreach`/`readlines` may invoke a
//! subprocess when the argument starts with `|`. Suggest `File.*` instead.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

const METHODS: &[&str] = &["read", "binread", "write", "binwrite", "foreach", "readlines"];

#[derive(Default)]
pub struct IoMethods;

impl IoMethods {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for IoMethods {
    fn name(&self) -> &'static str {
        "Security/IoMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node).into_owned();
        if !METHODS.contains(&method.as_str()) {
            return vec![];
        }
        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        // Receiver must be exactly `IO` (not `::IO`, per RuboCop's receiver.source == 'IO').
        let name = match m::constant_simple_name(&recv) {
            Some(n) => n,
            None => return vec![],
        };
        if name != "IO" {
            return vec![];
        }
        // Source of the receiver must be exactly 'IO' (no cbase, no namespace).
        let recv_loc = recv.location();
        let recv_src = &ctx.source[recv_loc.start_offset()..recv_loc.end_offset()];
        if recv_src != "IO" {
            return vec![];
        }

        // Check first argument: if it's a string literal starting with '|'
        // (after strip), skip (intentional subprocess).
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let first = match args.arguments().iter().next() {
            Some(a) => a,
            None => return vec![],
        };
        if let Some(s) = first.as_string_node() {
            let v = String::from_utf8_lossy(s.unescaped()).to_string();
            if v.trim_start().starts_with('|') {
                return vec![];
            }
        }

        let loc = node.location();
        // Offense range = call excluding block. Use closing paren end if present,
        // else the last arg's end.
        let end = node.closing_loc().map(|l| l.end_offset()).unwrap_or_else(|| {
            args.arguments().iter().last().map(|a| a.location().end_offset()).unwrap_or(loc.end_offset())
        });
        let msg = format!("`File.{}` is safer than `IO.{}`.", method, method);
        vec![ctx
            .offense_with_range(self.name(), &msg, Severity::Warning, loc.start_offset(), end)
            .with_correction(Correction::replace(
                recv_loc.start_offset(),
                recv_loc.end_offset(),
                "File".to_string(),
            ))]
    }
}

crate::register_cop!("Security/IoMethods", |_cfg| Some(Box::new(IoMethods::new())));
