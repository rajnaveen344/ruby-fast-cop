//! Bundler/InsecureProtocolSource cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

pub struct InsecureProtocolSource {
    allow_http_protocol: bool,
}

impl InsecureProtocolSource {
    pub fn new(allow_http_protocol: bool) -> Self {
        Self { allow_http_protocol }
    }
}

impl Default for InsecureProtocolSource {
    fn default() -> Self { Self::new(true) }
}

impl Cop for InsecureProtocolSource {
    fn name(&self) -> &'static str { "Bundler/InsecureProtocolSource" }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "source" {
            return vec![];
        }
        // bare call — no receiver
        if node.receiver().is_some() {
            return vec![];
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let first = match args.arguments().iter().next() {
            Some(a) => a,
            None => return vec![],
        };

        match &first {
            ruby_prism::Node::SymbolNode { .. } => {
                let sym = first.as_symbol_node().unwrap();
                let val = String::from_utf8_lossy(sym.unescaped());
                let name = val.as_ref();
                if name == "gemcutter" || name == "rubygems" || name == "rubyforge" {
                    let msg = format!(
                        "The source `:{name}` is deprecated because HTTP requests are insecure. \
                        Please change your source to 'https://rubygems.org' if possible, or 'http://rubygems.org' if not."
                    );
                    let loc = first.location();
                    vec![ctx.offense_with_range(self.name(), &msg, self.severity(), loc.start_offset(), loc.end_offset())]
                } else {
                    vec![]
                }
            }
            ruby_prism::Node::StringNode { .. } => {
                let s = first.as_string_node().unwrap();
                let val = String::from_utf8_lossy(s.unescaped());
                if val == "http://rubygems.org" {
                    if self.allow_http_protocol {
                        return vec![];
                    }
                    let loc = first.location();
                    vec![ctx.offense_with_range(
                        self.name(),
                        "Use `https://rubygems.org` instead of `http://rubygems.org`.",
                        self.severity(),
                        loc.start_offset(),
                        loc.end_offset(),
                    )]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct InsecureProtocolSourceCfg {
    allow_http_protocol: bool,
}

impl Default for InsecureProtocolSourceCfg {
    fn default() -> Self { Self { allow_http_protocol: true } }
}

crate::register_cop!("Bundler/InsecureProtocolSource", |cfg| {
    let c: InsecureProtocolSourceCfg = cfg.typed("Bundler/InsecureProtocolSource");
    Some(Box::new(InsecureProtocolSource::new(c.allow_http_protocol)))
});
