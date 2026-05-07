//! Style/ExpandPathArguments cop
//!
//! Checks for use of the File.expand_path arguments with __FILE__.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
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

        let (new_args_str) = match d {
            0 => "__FILE__".to_string(),
            1 => "__dir__".to_string(),
            _ => {
                let pp = Self::parent_path(&path_str);
                format!("'{}', __dir__", pp)
            }
        };

        let new_default_dir = if d <= 1 {
            if d == 0 { "__FILE__" } else { "__dir__" }
        } else {
            "__dir__"
        };
        let _ = new_default_dir;

        let msg = format!(
            "Use `expand_path({})` instead of `expand_path({}, __FILE__)`.",
            new_args_str,
            format!("'{}'", path_str)
        );

        // Offense on selector (expand_path)
        let msg_loc = node.message_loc()?;
        let start = msg_loc.start_offset();
        let end = msg_loc.end_offset();

        // Correction: replace args from open_paren+1 to close_paren-1 with new_args_str
        // node.arguments() gives us the args node; we need the full args range including parens
        let args_node = node.arguments()?;
        let args_start = args_node.location().start_offset();
        let args_end = args_node.location().end_offset();

        let mut offense = ctx.offense_with_range(
            "Style/ExpandPathArguments",
            &msg,
            Severity::Convention,
            start,
            end,
        );
        offense = offense.with_correction(Correction::replace(args_start, args_end, new_args_str));
        Some(offense)
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

        // Corrections:
        // 1. Replace __FILE__ arg to Pathname with __dir__
        // 2. Remove .parent — from call_operator_loc start to message_loc end of parent_call
        let file_node = &pn_args_list[0];
        let file_start = file_node.location().start_offset();
        let file_end = file_node.location().end_offset();

        // parent_call: receiver is pathname_call; we need to remove `.parent`
        // call_operator_loc is the `.` before parent, message_loc is `parent`
        let dot_start = parent_call.call_operator_loc()?.start_offset();
        let parent_end = parent_call.message_loc()?.end_offset();

        let correction = Correction {
            edits: vec![
                Edit { start_offset: file_start, end_offset: file_end, replacement: "__dir__".to_string() },
                Edit { start_offset: dot_start, end_offset: parent_end, replacement: "".to_string() },
            ],
        };

        let mut offense = ctx.offense_with_range(
            "Style/ExpandPathArguments",
            msg,
            Severity::Convention,
            start,
            end,
        );
        offense = offense.with_correction(correction);
        Some(offense)
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

        // Corrections:
        // 1. Replace __FILE__ arg to Pathname.new with __dir__
        // 2. Remove .parent
        let file_node = &new_args_list[0];
        let file_start = file_node.location().start_offset();
        let file_end = file_node.location().end_offset();

        let dot_start = parent_call.call_operator_loc()?.start_offset();
        let parent_end = parent_call.message_loc()?.end_offset();

        let correction = Correction {
            edits: vec![
                Edit { start_offset: file_start, end_offset: file_end, replacement: "__dir__".to_string() },
                Edit { start_offset: dot_start, end_offset: parent_end, replacement: "".to_string() },
            ],
        };

        let mut offense = ctx.offense_with_range(
            "Style/ExpandPathArguments",
            msg,
            Severity::Convention,
            start,
            end,
        );
        offense = offense.with_correction(correction);
        Some(offense)
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
