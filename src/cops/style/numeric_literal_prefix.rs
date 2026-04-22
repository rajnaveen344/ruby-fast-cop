//! Style/NumericLiteralPrefix cop
//!
//! Enforces consistent numeric literal prefixes (0o, 0x, 0b, no-prefix for decimal).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Visit, ProgramNode};

pub struct NumericLiteralPrefix {
    octal_style: OctalStyle,
}

#[derive(PartialEq, Clone, Copy)]
enum OctalStyle {
    ZeroWithO, // 0o prefix (default)
    ZeroOnly,  // 0 prefix (legacy)
}

impl Default for NumericLiteralPrefix {
    fn default() -> Self {
        Self { octal_style: OctalStyle::ZeroWithO }
    }
}

impl NumericLiteralPrefix {
    pub fn new(octal_style: OctalStyle) -> Self {
        Self { octal_style }
    }
}

impl Cop for NumericLiteralPrefix {
    fn name(&self) -> &'static str {
        "Style/NumericLiteralPrefix"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = PrefixVisitor {
            cop: self,
            ctx,
            offenses: &mut offenses,
        };
        visitor.visit(&result.node());
        offenses
    }
}

struct PrefixVisitor<'a, 'b> {
    cop: &'a NumericLiteralPrefix,
    ctx: &'b CheckContext<'b>,
    offenses: &'b mut Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for PrefixVisitor<'a, 'b> {
    fn visit_integer_node(&mut self, node: &ruby_prism::IntegerNode) {
        let loc = node.location();
        let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];

        if src.len() < 2 {
            return;
        }

        let msg = if src.starts_with("0O") || (src.starts_with("0o") && self.cop.octal_style == OctalStyle::ZeroOnly) {
            if self.cop.octal_style == OctalStyle::ZeroOnly {
                Some("Use 0 for octal literals.")
            } else {
                Some("Use 0o for octal literals.")
            }
        } else if src.starts_with("0O") {
            if self.cop.octal_style == OctalStyle::ZeroOnly {
                Some("Use 0 for octal literals.")
            } else {
                Some("Use 0o for octal literals.")
            }
        } else if src.starts_with("0o") && self.cop.octal_style == OctalStyle::ZeroOnly {
            Some("Use 0 for octal literals.")
        } else if src.starts_with("0X") {
            Some("Use 0x for hexadecimal literals.")
        } else if src.starts_with("0B") {
            Some("Use 0b for binary literals.")
        } else if src.starts_with("0d") || src.starts_with("0D") {
            Some("Do not use prefixes for decimal literals.")
        } else if src.len() > 1 && src.starts_with('0') && src.as_bytes()[1].is_ascii_digit()
            && self.cop.octal_style == OctalStyle::ZeroWithO
        {
            // Legacy octal `01234` and we want 0o
            Some("Use 0o for octal literals.")
        } else {
            None
        };

        if let Some(m) = msg {
            self.offenses.push(self.ctx.offense(self.cop.name(), m, self.cop.severity(), &loc));
        }
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_octal_style: String,
}

crate::register_cop!("Style/NumericLiteralPrefix", |cfg| {
    let c: Cfg = cfg.typed("Style/NumericLiteralPrefix");
    let style = if c.enforced_octal_style == "zero_only" {
        OctalStyle::ZeroOnly
    } else {
        OctalStyle::ZeroWithO
    };
    Some(Box::new(NumericLiteralPrefix::new(style)))
});
