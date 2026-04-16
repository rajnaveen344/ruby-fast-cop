//! Lint/DeprecatedClassMethods cop.
//!
//! Ported from https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/deprecated_class_methods.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct DeprecatedClassMethods;

impl DeprecatedClassMethods {
    pub fn new() -> Self { Self }
}

impl Cop for DeprecatedClassMethods {
    fn name(&self) -> &'static str { "Lint/DeprecatedClassMethods" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node).to_string();
        let recv_kind = classify_receiver(node.receiver().as_ref());

        let (offense_start, offense_end, prefer): (usize, usize, String) = match (recv_kind, method.as_str()) {
            // File.exists?(x), Dir.exists?(x) — require 1 arg
            (RecvKind::DirOrFile(ref name), "exists?") => {
                if arg_count(node) != 1 { return vec![]; }
                let sel = node.message_loc().unwrap();
                let start = node.location().start_offset();
                let end = sel.end_offset();
                (start, end, format!("{}.exist?", name))
            }
            // ENV.freeze / ENV.clone / ENV.dup — no-arg
            (RecvKind::Env(ref env_src), m @ ("freeze" | "clone" | "dup")) => {
                if arg_count(node) != 0 { return vec![]; }
                let sel = node.message_loc().unwrap();
                let start = node.location().start_offset();
                let end = sel.end_offset();
                let prefer = match m { "freeze" => "ENV".to_string(), _ => format!("{}.to_h", env_src) };
                (start, end, prefer)
            }
            // Socket.gethostbyaddr / Socket.gethostbyname — any args
            (RecvKind::Socket, m @ ("gethostbyaddr" | "gethostbyname")) => {
                let sel = node.message_loc().unwrap();
                let start = node.location().start_offset();
                let end = sel.end_offset();
                let prefer = if m == "gethostbyaddr" { "Addrinfo#getnameinfo".to_string() }
                             else { "Addrinfo.getaddrinfo".to_string() };
                (start, end, prefer)
            }
            // iterator? with no receiver, no args
            (RecvKind::None, "iterator?") => {
                if arg_count(node) != 0 { return vec![]; }
                let loc = node.location();
                (loc.start_offset(), loc.end_offset(), "block_given?".to_string())
            }
            // attr :name, true/false
            (RecvKind::None, "attr") => {
                let args = node.arguments();
                let arg_vec: Vec<Node> = args.as_ref().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                if arg_vec.len() != 2 { return vec![]; }
                let first = &arg_vec[0];
                let second = &arg_vec[1];
                let is_bool = matches!(second, Node::TrueNode { .. } | Node::FalseNode { .. });
                if !is_bool { return vec![]; }
                let is_true = matches!(second, Node::TrueNode { .. });
                let first_src = {
                    let l = first.location();
                    &ctx.source[l.start_offset()..l.end_offset()]
                };
                let replacement_method = if is_true { "attr_accessor" } else { "attr_reader" };
                let prefer = format!("{} {}", replacement_method, first_src);
                let loc = node.location();
                (loc.start_offset(), loc.end_offset(), prefer)
            }
            _ => return vec![],
        };

        let current_src = &ctx.source[offense_start..offense_end];
        let msg = format!("`{}` is deprecated in favor of `{}`.", current_src, prefer);

        // Build correction
        let correction = match (classify_receiver(node.receiver().as_ref()), method.as_str()) {
            (RecvKind::Socket, _) => None, // no autocorrect for socket
            (RecvKind::Env(_), "freeze") => {
                let loc = node.location();
                Some(Correction::replace(loc.start_offset(), loc.end_offset(), "ENV"))
            }
            _ => Some(Correction::replace(offense_start, offense_end, prefer.clone())),
        };

        let mut off = ctx.offense_with_range(
            "Lint/DeprecatedClassMethods",
            &msg,
            Severity::Warning,
            offense_start,
            offense_end,
        );
        if let Some(c) = correction { off = off.with_correction(c); }
        vec![off]
    }
}

fn arg_count(node: &ruby_prism::CallNode) -> usize {
    node.arguments().map_or(0, |a| a.arguments().iter().count())
}

#[derive(Debug)]
enum RecvKind {
    None,
    DirOrFile(String), // full receiver source ("File" or "::File")
    Env(String),
    Socket,
    Other,
}

fn classify_receiver(recv: Option<&Node>) -> RecvKind {
    let r = match recv { Some(r) => r, None => return RecvKind::None };
    match r {
        Node::ConstantReadNode { .. } => {
            let name = node_name!(r.as_constant_read_node().unwrap()).to_string();
            match name.as_str() {
                "ENV" => RecvKind::Env("ENV".to_string()),
                "Socket" => RecvKind::Socket,
                "File" | "Dir" => RecvKind::DirOrFile(name),
                _ => RecvKind::Other,
            }
        }
        Node::ConstantPathNode { .. } => {
            // `::File`, `::Dir`, `::ENV`, `::Socket` — parent is nil (cbase), child is the constant.
            let cp = r.as_constant_path_node().unwrap();
            if cp.parent().is_some() { return RecvKind::Other; }
            let name_id = match cp.name() { Some(n) => n, None => return RecvKind::Other };
            let name = String::from_utf8_lossy(name_id.as_slice()).to_string();
            let src = format!("::{}", name);
            match name.as_str() {
                "ENV" => RecvKind::Env(src),
                "Socket" => RecvKind::Socket,
                "File" | "Dir" => RecvKind::DirOrFile(src),
                _ => RecvKind::Other,
            }
        }
        _ => RecvKind::Other,
    }
}
