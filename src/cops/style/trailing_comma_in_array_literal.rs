use crate::cops::{CheckContext, Cop};
use crate::helpers::trailing_comma;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

pub use crate::helpers::trailing_comma::EnforcedStyleForMultiline;

pub struct TrailingCommaInArrayLiteral {
    style: EnforcedStyleForMultiline,
}

impl TrailingCommaInArrayLiteral {
    pub fn new(style: EnforcedStyleForMultiline) -> Self {
        Self { style }
    }
}

impl Cop for TrailingCommaInArrayLiteral {
    fn name(&self) -> &'static str {
        trailing_comma::ARRAY.cop_name
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        let elements: Vec<Node> = node.elements().iter().collect();
        if elements.is_empty() {
            return vec![];
        }

        // Need `[...]` delimiter. Skip %w/%i/%W/%I percent literals.
        let (open_loc, close_loc) = match (node.opening_loc(), node.closing_loc()) {
            (Some(o), Some(c)) => (o, c),
            _ => return vec![],
        };
        let open = open_loc.start_offset();
        let close = close_loc.start_offset();
        let bytes = ctx.source.as_bytes();
        if bytes.get(open).copied() != Some(b'[') {
            return vec![];
        }

        trailing_comma::check(
            ctx,
            trailing_comma::ARRAY,
            self.style,
            &elements,
            open,
            close,
            false,
        )
    }
}

crate::register_cop!("Style/TrailingCommaInArrayLiteral", |cfg| {
    let cop_config = cfg.get_cop_config("Style/TrailingCommaInArrayLiteral");
    let style = cop_config
        .and_then(|c| c.raw.get("EnforcedStyleForMultiline"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "comma" => EnforcedStyleForMultiline::Comma,
            "consistent_comma" => EnforcedStyleForMultiline::ConsistentComma,
            "diff_comma" => EnforcedStyleForMultiline::DiffComma,
            _ => EnforcedStyleForMultiline::NoComma,
        })
        .unwrap_or(EnforcedStyleForMultiline::NoComma);
    Some(Box::new(TrailingCommaInArrayLiteral::new(style)))
});
