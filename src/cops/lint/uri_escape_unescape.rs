//! Lint/UriEscapeUnescape cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/uri_escape_unescape.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct UriEscapeUnescape;

impl UriEscapeUnescape {
    pub fn new() -> Self { Self }
}

const ESCAPE_METHODS: &[&str] = &["escape", "encode"];
const UNESCAPE_METHODS: &[&str] = &["unescape", "decode"];

const ESCAPE_MSG: &str = "use `CGI.escape`, `URI.encode_www_form` or `URI.encode_www_form_component` depending on your specific use case.";
const UNESCAPE_MSG: &str = "use `CGI.unescape`, `URI.decode_www_form` or `URI.decode_www_form_component` depending on your specific use case.";

impl Cop for UriEscapeUnescape {
    fn name(&self) -> &'static str { "Lint/UriEscapeUnescape" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let is_escape = ESCAPE_METHODS.iter().any(|m| *m == method);
        let is_unescape = UNESCAPE_METHODS.iter().any(|m| *m == method);
        if !is_escape && !is_unescape {
            return vec![];
        }

        // Receiver must be URI or ::URI
        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let (is_uri, recv_src) = match &recv {
            Node::ConstantReadNode { .. } => {
                let name = node_name!(recv.as_constant_read_node().unwrap());
                let is = name == "URI";
                let s = ctx.src(recv.location().start_offset(), recv.location().end_offset()).to_string();
                (is, s)
            }
            Node::ConstantPathNode { .. } => {
                let cp = recv.as_constant_path_node().unwrap();
                let name = cp.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string()).unwrap_or_default();
                // ::URI has no parent constant
                if name == "URI" && cp.parent().is_none() {
                    let s = ctx.src(recv.location().start_offset(), recv.location().end_offset()).to_string();
                    (true, s)
                } else {
                    (false, String::new())
                }
            }
            _ => (false, String::new()),
        };

        if !is_uri {
            return vec![];
        }

        // Build message like: "`URI.escape` method is obsolete..."
        let full_msg = format!(
            "`{}.{}` method is obsolete and should not be used. Instead, {}",
            recv_src,
            method,
            if is_escape { ESCAPE_MSG } else { UNESCAPE_MSG }
        );

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        vec![ctx.offense_with_range(
            "Lint/UriEscapeUnescape",
            &full_msg,
            Severity::Warning,
            start,
            end,
        )]
    }
}

crate::register_cop!("Lint/UriEscapeUnescape", |_cfg| {
    Some(Box::new(UriEscapeUnescape::new()))
});
