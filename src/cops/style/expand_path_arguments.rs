//! Style/ExpandPathArguments cop
//!
//! Checks for use of the File.expand_path arguments with __FILE__.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Node};

#[derive(Default)]
pub struct ExpandPathArguments;

impl ExpandPathArguments {
    pub fn new() -> Self {
        Self
    }

    fn is_file_magic(node: &Node) -> bool {
        matches!(node, Node::SourceFileNode { .. })
    }

    fn is_str_node(node: &Node) -> bool {
        matches!(node, Node::StringNode { .. })
    }

    fn str_content(node: &Node, source: &str) -> Option<String> {
        if let Some(s) = node.as_string_node() {
            let start = s.location().start_offset();
            let end = s.location().end_offset();
            let raw = &source[start..end];
            // Strip surrounding quotes
            let inner = if (raw.starts_with('\'') && raw.ends_with('\''))
                || (raw.starts_with('"') && raw.ends_with('"'))
            {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw.to_string()
            };
            Some(inner)
        } else {
            None
        }
    }

    fn depth(path: &str) -> usize {
        path.split('/').filter(|p| *p != ".").count()
    }

    fn parent_path(path: &str) -> String {
        let mut parts: Vec<&str> = path.split('/').filter(|p| *p != ".").collect();
        // Remove first `..`
        if let Some(pos) = parts.iter().position(|&p| p == "..") {
            parts.remove(pos);
        }
        parts.join("/")
    }

    /// Check `File.expand_path(path_str, __FILE__)` pattern
    fn check_file_expand_path(node: &CallNode, ctx: &CheckContext) -> Option<Offense> {
        let method = node_name!(node);
        if method != "expand_path" {
            return None;
        }

        // Receiver must be File or ::File constant
        let receiver = node.receiver()?;
        let is_file_const = match &receiver {
            Node::ConstantReadNode { .. } => {
                let cr = receiver.as_constant_read_node().unwrap();
                String::from_utf8_lossy(cr.name().as_slice()) == "File"
            }
            Node::ConstantPathNode { .. } => {
                let cp = receiver.as_constant_path_node().unwrap();
                if let Some(name_id) = cp.name() {
                    String::from_utf8_lossy(name_id.as_slice()) == "File"
                } else {
                    false
                }
            }
            _ => false,
        };
        if !is_file_const {
            return None;
        }

        let args = node.arguments()?;
        let args_list: Vec<_> = args.arguments().iter().collect();
        if args_list.len() != 2 {
            return None;
        }

        let current_path_node = &args_list[0];
        let default_dir_node = &args_list[1];

        // Second arg must be __FILE__
        if !Self::is_file_magic(default_dir_node) {
            return None;
        }

        // First arg must be a string literal
        if !Self::is_str_node(current_path_node) {
            return None;
        }

        let path_str = Self::str_content(current_path_node, ctx.source)?;
        let d = Self::depth(&path_str);

        let (new_path, new_default_dir) = match d {
            0 => ("".to_string(), "__FILE__".to_string()),
            1 => ("".to_string(), "__dir__".to_string()),
            _ => {
                let pp = Self::parent_path(&path_str);
                (format!("'{}', ", pp), "__dir__".to_string())
            }
        };

        let msg = format!(
            "Use `expand_path({}{})` instead of `expand_path({}, __FILE__)`.",
            new_path,
            new_default_dir,
            format!("'{}'", path_str)
        );

        // Offense on selector (expand_path)
        let msg_loc = node.message_loc()?;
        let start = msg_loc.start_offset();
        let end = msg_loc.end_offset();

        Some(ctx.offense_with_range(
            "Style/ExpandPathArguments",
            &msg,
            Severity::Convention,
            start,
            end,
        ))
    }

    /// Check `Pathname(__FILE__).parent.expand_path` pattern
    fn check_pathname_expand_path(node: &CallNode, ctx: &CheckContext) -> Option<Offense> {
        let method = node_name!(node);
        if method != "expand_path" {
            return None;
        }
        if node.receiver().is_none() {
            return None;
        }

        let receiver = node.receiver().unwrap();
        // receiver should be `Pathname(__FILE__).parent` call
        let parent_call = receiver.as_call_node()?;
        let parent_method = node_name!(parent_call);
        if parent_method != "parent" {
            return None;
        }

        let pathname_call = parent_call.receiver()?.as_call_node()?;
        let pathname_method = node_name!(pathname_call);
        if pathname_method != "Pathname" {
            return None;
        }

        // Pathname must have receiver == nil (bare call)
        if pathname_call.receiver().is_some() {
            return None;
        }

        // Arg to Pathname must be __FILE__
        let pn_args = pathname_call.arguments()?;
        let pn_args_list: Vec<_> = pn_args.arguments().iter().collect();
        if pn_args_list.len() != 1 || !Self::is_file_magic(&pn_args_list[0]) {
            return None;
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let msg = "Use `Pathname(__dir__).expand_path` instead of `Pathname(__FILE__).parent.expand_path`.";

        Some(ctx.offense_with_range(
            "Style/ExpandPathArguments",
            msg,
            Severity::Convention,
            start,
            end,
        ))
    }

    /// Check `Pathname.new(__FILE__).parent.expand_path` pattern
    fn check_pathname_new_expand_path(node: &CallNode, ctx: &CheckContext) -> Option<Offense> {
        let method = node_name!(node);
        if method != "expand_path" {
            return None;
        }
        if node.receiver().is_none() {
            return None;
        }

        let receiver = node.receiver().unwrap();
        let parent_call = receiver.as_call_node()?;
        let parent_method = node_name!(parent_call);
        if parent_method != "parent" {
            return None;
        }

        let new_call = parent_call.receiver()?.as_call_node()?;
        let new_method = node_name!(new_call);
        if new_method != "new" {
            return None;
        }

        // new_call receiver must be Pathname or ::Pathname
        let pn_receiver = new_call.receiver()?;
        let is_pathname = match &pn_receiver {
            Node::ConstantReadNode { .. } => {
                let cr = pn_receiver.as_constant_read_node().unwrap();
                String::from_utf8_lossy(cr.name().as_slice()) == "Pathname"
            }
            Node::ConstantPathNode { .. } => {
                let cp = pn_receiver.as_constant_path_node().unwrap();
                if let Some(name_id) = cp.name() {
                    String::from_utf8_lossy(name_id.as_slice()) == "Pathname"
                } else {
                    false
                }
            }
            _ => false,
        };
        if !is_pathname {
            return None;
        }

        let new_args = new_call.arguments()?;
        let new_args_list: Vec<_> = new_args.arguments().iter().collect();
        if new_args_list.len() != 1 || !Self::is_file_magic(&new_args_list[0]) {
            return None;
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let msg = "Use `Pathname.new(__dir__).expand_path` instead of `Pathname.new(__FILE__).parent.expand_path`.";

        Some(ctx.offense_with_range(
            "Style/ExpandPathArguments",
            msg,
            Severity::Convention,
            start,
            end,
        ))
    }
}

impl Cop for ExpandPathArguments {
    fn name(&self) -> &'static str {
        "Style/ExpandPathArguments"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Try File.expand_path first
        if let Some(offense) = Self::check_file_expand_path(node, ctx) {
            return vec![offense];
        }
        // Try Pathname.new(__FILE__).parent.expand_path
        if let Some(offense) = Self::check_pathname_new_expand_path(node, ctx) {
            return vec![offense];
        }
        // Try Pathname(__FILE__).parent.expand_path
        if let Some(offense) = Self::check_pathname_expand_path(node, ctx) {
            return vec![offense];
        }
        vec![]
    }
}

crate::register_cop!("Style/ExpandPathArguments", |_cfg| {
    Some(Box::new(ExpandPathArguments::new()))
});
