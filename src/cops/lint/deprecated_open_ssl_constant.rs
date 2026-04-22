//! Lint/DeprecatedOpenSSLConstant - Algorithmic constants for OpenSSL deprecated since v2.2.0.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Use `{constant}.{method}({replacement_args})` instead of `{original}`.";

const NO_ARG_ALGORITHM: &[&str] = &["BF", "DES", "IDEA", "RC4"];

#[derive(Default)]
pub struct DeprecatedOpenSSLConstant;

impl DeprecatedOpenSSLConstant {
    pub fn new() -> Self { Self }
}

impl Cop for DeprecatedOpenSSLConstant {
    fn name(&self) -> &'static str { "Lint/DeprecatedOpenSSLConstant" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        let method_str = method.as_ref();

        if method_str != "new" && method_str != "digest" {
            return;
        }

        // The receiver must be OpenSSL::Cipher::<Algo> or OpenSSL::Digest::<Algo>
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        // Check for OpenSSL::Digest::Digest.xxx — no-op (not flagged)
        // and OpenSSL::Cipher::Cipher.xxx — flagged (corrected to OpenSSL::Cipher.new(arg))

        let algo_info = match extract_algo_info(&receiver, self.ctx.source) {
            Some(a) => a,
            None => return,
        };

        // Guard: if receiver is OpenSSL::Digest::Digest, skip (not flagged in RuboCop)
        // But OpenSSL::Cipher::Cipher IS flagged
        if algo_info.class == "Digest" && algo_info.algo_name == "Digest" {
            return;
        }

        // Check arguments: if any arg is a variable, method call, or constant → skip
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() {
                match &arg {
                    Node::LocalVariableReadNode { .. }
                    | Node::InstanceVariableReadNode { .. }
                    | Node::ClassVariableReadNode { .. }
                    | Node::GlobalVariableReadNode { .. }
                    | Node::CallNode { .. }
                    | Node::ConstantReadNode { .. }
                    | Node::ConstantPathNode { .. } => return,
                    _ => {}
                }
            }
        }

        // Build message and correction
        let original_src = &self.ctx.source[node.location().start_offset()..node.location().end_offset()];
        let replacement_args = build_replacement_args(node, &algo_info, self.ctx.source);
        let parent_constant = format!("OpenSSL::{}", algo_info.class);

        let msg = format!(
            "Use `{}.{}({})` instead of `{}`.",
            parent_constant, method_str, replacement_args, original_src
        );

        let correction = build_correction(node, &algo_info, method_str, &replacement_args, self.ctx.source);

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        let mut offense = self.ctx.offense_with_range(
            "Lint/DeprecatedOpenSSLConstant",
            &msg,
            Severity::Warning,
            start,
            end,
        );
        offense.correction = correction;
        self.offenses.push(offense);
    }
}

#[derive(Debug)]
struct AlgoInfo {
    /// "Cipher" or "Digest"
    class: String,
    /// e.g. "AES", "AES128", "SHA256", "BF", "Cipher"
    algo_name: String,
}

/// Extract algo info from a ConstantPathNode receiver like OpenSSL::Cipher::AES
fn extract_algo_info(node: &Node, source: &str) -> Option<AlgoInfo> {
    // Must be a ConstantPathNode: OpenSSL::Cipher::AES
    let path = node.as_constant_path_node()?;

    let algo_name_bytes = path.name()?;
    let algo_name = String::from_utf8_lossy(algo_name_bytes.as_slice()).into_owned();

    // Parent must be OpenSSL::Cipher or OpenSSL::Digest
    let parent = path.parent()?;
    let parent_path = parent.as_constant_path_node()?;

    let class_bytes = parent_path.name()?;
    let class = String::from_utf8_lossy(class_bytes.as_slice()).into_owned();

    if class != "Cipher" && class != "Digest" {
        return None;
    }

    // Parent of parent must be OpenSSL constant
    let grandparent = parent_path.parent()?;
    let grandparent_name = match &grandparent {
        Node::ConstantReadNode { .. } => {
            let c = grandparent.as_constant_read_node()?;
            String::from_utf8_lossy(c.name().as_slice()).into_owned()
        }
        Node::ConstantPathNode { .. } => {
            // Could be ::OpenSSL — get source
            let src = &source[grandparent.location().start_offset()..grandparent.location().end_offset()];
            src.trim_start_matches("::").to_string()
        }
        _ => return None,
    };

    if grandparent_name != "OpenSSL" {
        return None;
    }

    Some(AlgoInfo { class, algo_name })
}

fn build_replacement_args(node: &ruby_prism::CallNode, algo: &AlgoInfo, source: &str) -> String {
    let method = String::from_utf8_lossy(node.name().as_slice());
    let method_str = method.as_ref();

    // Get arguments
    let args: Vec<Node> = node.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();

    if algo.class == "Cipher" {
        if algo.algo_name == "Cipher" {
            // OpenSSL::Cipher::Cipher.new(arg) → arg's source
            if let Some(arg) = args.first() {
                let src = &source[arg.location().start_offset()..arg.location().end_offset()];
                return src.to_string();
            }
            return String::new();
        }
        build_cipher_args(&algo.algo_name, &args, source)
    } else {
        // Digest
        let algo_quoted = format!("'{}'", algo.algo_name);
        if method_str == "digest" {
            let rest: Vec<String> = args.iter().map(|a| {
                source[a.location().start_offset()..a.location().end_offset()].to_string()
            }).collect();
            if rest.is_empty() {
                algo_quoted
            } else {
                format!("{}, {}", algo_quoted, rest.join(", "))
            }
        } else {
            // new
            algo_quoted
        }
    }
}

fn build_cipher_args(algo_name: &str, args: &[Node], source: &str) -> String {
    // Parse algorithm name into parts (e.g. "AES128" → ["aes", "128"], "AES" → ["aes"])
    let algo_parts = parse_algo_name(algo_name);
    let algo_upper = algo_name.to_uppercase();
    let no_arg_algo = NO_ARG_ALGORITHM.contains(&algo_upper.as_str());

    if no_arg_algo && args.is_empty() {
        return format!("'{}'", algo_name.to_lowercase());
    }

    let size_and_mode: Vec<String> = sanitize_arguments(args, source);
    let mode = if size_and_mode.is_empty() { vec!["cbc".to_string()] } else { vec![] };

    let combined: Vec<String> = algo_parts.iter()
        .map(|s| s.to_lowercase())
        .chain(size_and_mode.iter().map(|s| s.to_lowercase()))
        .chain(mode.into_iter())
        .take(3)
        .collect();

    format!("'{}'", combined.join("-"))
}

fn parse_algo_name(name: &str) -> Vec<String> {
    // e.g. "AES128" → ["AES", "128"], "AES" → ["AES"], "AES256" → ["AES", "256"]
    // Scan in groups of 3 chars as RuboCop does: name.scan(/.{3}/).join('-')
    // Actually RuboCop uses scan(/.{3}/) which gives chunks of 3
    let chunks: Vec<String> = name.as_bytes()
        .chunks(3)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    chunks
}

fn sanitize_arguments(args: &[Node], source: &str) -> Vec<String> {
    let mut result = Vec::new();
    for arg in args {
        let raw = match arg {
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => {
                // Get string value without quotes
                let src = &source[arg.location().start_offset()..arg.location().end_offset()];
                // Strip quotes
                src.trim_matches('"').trim_matches('\'').to_string()
            }
            Node::SymbolNode { .. } => {
                let src = &source[arg.location().start_offset()..arg.location().end_offset()];
                // Strip leading `:` and quotes
                src.trim_start_matches(':').trim_matches('\'').trim_matches('"').to_string()
            }
            _ => {
                let src = &source[arg.location().start_offset()..arg.location().end_offset()];
                src.to_string()
            }
        };
        // Remove colons, quotes, split on '-'
        let cleaned = raw.replace([':', '\'', '"'], "");
        for part in cleaned.split('-') {
            if !part.is_empty() {
                result.push(part.to_string());
            }
        }
    }
    result
}

fn build_correction(
    node: &ruby_prism::CallNode,
    algo: &AlgoInfo,
    method_str: &str,
    replacement_args: &str,
    source: &str,
) -> Option<Correction> {
    // We need to:
    // 1. Remove "::AlgoName" from the receiver (OpenSSL::Cipher::AES → OpenSSL::Cipher)
    // 2. Replace ".new(old_args)" with ".new(new_args)"

    let receiver = node.receiver()?;

    // The ConstantPathNode for OpenSSL::Cipher::AES
    let path = receiver.as_constant_path_node()?;

    // Get the :: before AlgoName and the AlgoName itself
    // path.delimiter_loc() gives the :: before the last name
    // We need to remove from the delimiter to the end of the receiver
    let parent = path.parent()?;

    // The range to remove: from end of parent (OpenSSL::Cipher) to end of receiver
    let parent_end = parent.location().end_offset();
    let receiver_end = receiver.location().end_offset();

    // Remove "::AlgoName" (from parent_end to receiver_end)
    // Then replace from dot onwards
    let dot_loc = node.call_operator_loc()?;
    let dot_end = dot_loc.end_offset();
    let node_end = node.location().end_offset();

    // Two edits:
    // 1. Delete "::AlgoName" (parent_end..receiver_end)
    // 2. Replace ".new(old_args)" with ".new(new_args)" (dot_end-1..node_end)

    let new_method_call = format!("{method_str}({replacement_args})");

    use crate::offense::Edit;

    let edits = vec![
        Edit { start_offset: parent_end, end_offset: receiver_end, replacement: String::new() },
        Edit { start_offset: dot_end, end_offset: node_end, replacement: new_method_call },
    ];

    // Also need to include the `.` itself or the space before method name?
    // dot_loc is the `.` operator. dot_end is position after `.`.
    // From dot_end to node_end: `new(...)` or `new` (without args)
    // We want to replace `new(...)` with `new(replacement_args)`.
    let _ = source;

    Some(crate::offense::Correction { edits })
}

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/DeprecatedOpenSSLConstant", |_cfg| Some(Box::new(DeprecatedOpenSSLConstant::new())));
