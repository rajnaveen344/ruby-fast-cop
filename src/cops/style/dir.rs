use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Use `__dir__` to get an absolute path to the current file's directory.";

#[derive(Default)]
pub struct Dir;

impl Dir {
    pub fn new() -> Self {
        Self
    }

    /// Check if a node is `File` or `::File` constant
    fn is_file_const(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                node.as_constant_read_node().unwrap().name().as_slice() == b"File"
            }
            Node::ConstantPathNode { .. } => {
                let p = node.as_constant_path_node().unwrap();
                if p.parent().is_some() {
                    return false;
                }
                p.name().map_or(false, |id| id.as_slice() == b"File")
            }
            _ => false,
        }
    }

    /// Check if node is `__FILE__`
    fn is_file_keyword(node: &Node) -> bool {
        matches!(node, Node::SourceFileNode { .. })
    }

    /// Check if node is `File.dirname(__FILE__)` or `::File.dirname(__FILE__)`
    fn is_file_dirname_file(node: &Node) -> bool {
        let call = match node.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        if node_name!(call) != "dirname" {
            return false;
        }
        let recv = match call.receiver() {
            Some(r) => r,
            None => return false,
        };
        if !Self::is_file_const(&recv) {
            return false;
        }
        let args = match call.arguments() {
            Some(a) => a,
            None => return false,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        arg_list.len() == 1 && Self::is_file_keyword(&arg_list[0])
    }

    /// Check if node is `File.realpath(__FILE__)` or `::File.realpath(__FILE__)`
    fn is_file_realpath_file(node: &Node) -> bool {
        let call = match node.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        if node_name!(call) != "realpath" {
            return false;
        }
        let recv = match call.receiver() {
            Some(r) => r,
            None => return false,
        };
        if !Self::is_file_const(&recv) {
            return false;
        }
        let args = match call.arguments() {
            Some(a) => a,
            None => return false,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        arg_list.len() == 1 && Self::is_file_keyword(&arg_list[0])
    }
}

impl Cop for Dir {
    fn name(&self) -> &'static str {
        "Style/Dir"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);

        // Pattern 1: File.expand_path(File.dirname(__FILE__))
        if method == "expand_path" {
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            if !Self::is_file_const(&recv) {
                return vec![];
            }
            let args = match node.arguments() {
                Some(a) => a,
                None => return vec![],
            };
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1 && Self::is_file_dirname_file(&arg_list[0]) {
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                return vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)];
            }
        }

        // Pattern 2: File.dirname(File.realpath(__FILE__))
        if method == "dirname" {
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            if !Self::is_file_const(&recv) {
                return vec![];
            }
            let args = match node.arguments() {
                Some(a) => a,
                None => return vec![],
            };
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1 && Self::is_file_realpath_file(&arg_list[0]) {
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                return vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)];
            }
        }

        vec![]
    }
}

crate::register_cop!("Style/Dir", |_cfg| Some(Box::new(Dir::new())));
