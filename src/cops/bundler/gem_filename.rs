//! Bundler/GemFilename cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

pub struct GemFilename {
    enforced_style: GemStyle,
}

#[derive(Clone, Copy, PartialEq)]
pub enum GemStyle {
    Gemfile,
    GemsRb,
}

impl GemFilename {
    pub fn new(enforced_style: GemStyle) -> Self { Self { enforced_style } }

    fn global_offense(&self, ctx: &CheckContext, msg: &str) -> Offense {
        // Global offense: line 1, col 0, end col 0 (zero-width, not widened)
        let location = Location::new(1, 0, 1, 0);
        Offense::new(self.name(), msg, self.severity(), location, ctx.filename)
    }
}

impl Default for GemFilename {
    fn default() -> Self { Self::new(GemStyle::Gemfile) }
}

impl Cop for GemFilename {
    fn name(&self) -> &'static str { "Bundler/GemFilename" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Get the basename from the filename path
        let path = ctx.filename;
        let basename = path.rsplit('/').next().unwrap_or(path);

        match self.enforced_style {
            GemStyle::Gemfile => {
                match basename {
                    "gems.rb" => {
                        let msg = format!(
                            "`gems.rb` file was found but `Gemfile` is required (file path: {path})."
                        );
                        vec![self.global_offense(ctx, &msg)]
                    }
                    "gems.locked" => {
                        let msg = format!(
                            "Expected a `Gemfile.lock` with `Gemfile` but found `gems.locked` file (file path: {path})."
                        );
                        vec![self.global_offense(ctx, &msg)]
                    }
                    "Gemfile" | "Gemfile.lock" => vec![],
                    _ => vec![],
                }
            }
            GemStyle::GemsRb => {
                match basename {
                    "Gemfile" => {
                        let msg = format!(
                            "`Gemfile` was found but `gems.rb` file is required (file path: {path})."
                        );
                        vec![self.global_offense(ctx, &msg)]
                    }
                    "Gemfile.lock" => {
                        let msg = format!(
                            "Expected a `gems.locked` file with `gems.rb` but found `Gemfile.lock` (file path: {path})."
                        );
                        vec![self.global_offense(ctx, &msg)]
                    }
                    "gems.rb" | "gems.locked" => vec![],
                    _ => vec![],
                }
            }
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct GemFilenameCfg {
    enforced_style: String,
}

impl Default for GemFilenameCfg {
    fn default() -> Self { Self { enforced_style: "Gemfile".to_string() } }
}

crate::register_cop!("Bundler/GemFilename", |cfg| {
    let c: GemFilenameCfg = cfg.typed("Bundler/GemFilename");
    let style = if c.enforced_style == "gems.rb" { GemStyle::GemsRb } else { GemStyle::Gemfile };
    Some(Box::new(GemFilename::new(style)))
});
