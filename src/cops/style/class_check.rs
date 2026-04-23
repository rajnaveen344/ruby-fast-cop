use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

const MSG: &str = "Prefer `Object#%s` over `Object#%s`.";

pub struct ClassCheck {
    enforced_style: String,
}

impl Default for ClassCheck {
    fn default() -> Self {
        Self {
            enforced_style: "is_a?".to_string(),
        }
    }
}

impl ClassCheck {
    pub fn new(enforced_style: String) -> Self {
        Self { enforced_style }
    }
}

impl Cop for ClassCheck {
    fn name(&self) -> &'static str {
        "Style/ClassCheck"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "is_a?" && method != "kind_of?" {
            return vec![];
        }
        // If method matches enforced style, it's good
        if method == self.enforced_style {
            return vec![];
        }
        let (prefer, current) = if method == "is_a?" {
            ("kind_of?", "is_a?")
        } else {
            ("is_a?", "kind_of?")
        };
        let msg = format!("Prefer `Object#{}` over `Object#{}`.", prefer, current);
        let sel = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        vec![ctx.offense(self.name(), &msg, self.severity(), &sel)]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/ClassCheck", |cfg| {
    let c: Cfg = cfg.typed("Style/ClassCheck");
    let style = c.enforced_style.unwrap_or_else(|| "is_a?".to_string());
    Some(Box::new(ClassCheck::new(style)))
});
