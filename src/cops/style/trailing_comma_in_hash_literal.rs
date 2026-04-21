use crate::cops::{CheckContext, Cop};
use crate::helpers::trailing_comma;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

pub use crate::helpers::trailing_comma::EnforcedStyleForMultiline;

pub struct TrailingCommaInHashLiteral {
    style: EnforcedStyleForMultiline,
}

impl TrailingCommaInHashLiteral {
    pub fn new(style: EnforcedStyleForMultiline) -> Self {
        Self { style }
    }
}

impl Cop for TrailingCommaInHashLiteral {
    fn name(&self) -> &'static str {
        trailing_comma::HASH.cop_name
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        let elements: Vec<Node> = node.elements().iter().collect();
        if elements.is_empty() {
            return vec![];
        }

        let open = node.opening_loc().start_offset();
        let close = node.closing_loc().start_offset();

        trailing_comma::check(
            ctx,
            trailing_comma::HASH,
            self.style,
            &elements,
            open,
            close,
            false,
        )
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style_for_multiline: String }

crate::register_cop!("Style/TrailingCommaInHashLiteral", |cfg| {
    let c: Cfg = cfg.typed("Style/TrailingCommaInHashLiteral");
    let style = match c.enforced_style_for_multiline.as_str() {
        "comma" => EnforcedStyleForMultiline::Comma,
        "consistent_comma" => EnforcedStyleForMultiline::ConsistentComma,
        "diff_comma" => EnforcedStyleForMultiline::DiffComma,
        _ => EnforcedStyleForMultiline::NoComma,
    };
    Some(Box::new(TrailingCommaInHashLiteral::new(style)))
});
