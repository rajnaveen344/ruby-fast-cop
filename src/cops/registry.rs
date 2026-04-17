//! Cop auto-registration via the `inventory` crate.
//!
//! Each cop file registers itself once, at the bottom:
//!
//! ```ignore
//! crate::register_cop!("Lint/Debugger", |_cfg| Some(Box::new(Debugger::new())));
//! ```
//!
//! For cops with configuration, the factory closure reads raw YAML off the
//! `Config` and builds the cop:
//!
//! ```ignore
//! crate::register_cop!("Lint/AssignmentInCondition", |cfg| {
//!     let allow = cfg.get_cop_config("Lint/AssignmentInCondition")
//!         .and_then(|c| c.allow_safe_assignment).unwrap_or(true);
//!     Some(Box::new(AssignmentInCondition::new(allow)))
//! });
//! ```
//!
//! The registry replaces the ~5000-LOC `build_cops_from_config` +
//! `build_single_cop` in `lib.rs` and the `all()` list in `cops/mod.rs`.
//! Adding a cop now touches only one file.

use crate::config::Config;
use crate::cops::Cop;

/// One entry per cop, collected at link-time by `inventory`.
pub struct Registration {
    pub name: &'static str,
    /// Factory: given full config, produce a configured cop (or `None` if
    /// the cop decides to opt out based on config).
    pub factory: fn(&Config) -> Option<Box<dyn Cop>>,
}

inventory::collect!(Registration);

/// Build all enabled cops from a config. Replaces `build_cops_from_config`.
pub fn build_from_config(config: &Config) -> Vec<Box<dyn Cop>> {
    inventory::iter::<Registration>
        .into_iter()
        .filter(|r| config.is_cop_enabled(r.name))
        .filter_map(|r| (r.factory)(config))
        .collect()
}

/// Build a single cop by name. Replaces `build_single_cop`.
pub fn build_one(name: &str, config: &Config) -> Option<Box<dyn Cop>> {
    inventory::iter::<Registration>
        .into_iter()
        .find(|r| r.name == name)
        .and_then(|r| (r.factory)(config))
}

/// Build every registered cop with default config (no enable/disable filter).
/// Replaces `cops::all()`.
pub fn all_with_defaults() -> Vec<Box<dyn Cop>> {
    let empty = Config::default();
    inventory::iter::<Registration>
        .into_iter()
        .filter_map(|r| (r.factory)(&empty))
        .collect()
}

/// Registration macro. See module docs for usage.
#[macro_export]
macro_rules! register_cop {
    ($name:literal, $factory:expr) => {
        inventory::submit! {
            $crate::cops::registry::Registration {
                name: $name,
                factory: $factory,
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_finds_migrated_cops() {
        // These cops are migrated to register_cop! below. As migration
        // proceeds, add more assertions here.
        let all: Vec<&str> = inventory::iter::<Registration>
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert!(all.contains(&"Lint/Debugger"), "Lint/Debugger missing from registry");
        assert!(all.contains(&"Style/RedundantFreeze"), "Style/RedundantFreeze missing");
    }

    #[test]
    fn build_from_config_respects_enabled_flag() {
        use crate::config::{Config, CopConfig};
        use std::collections::HashMap;

        let mut cfg = Config::default();
        cfg.cops.insert(
            "Lint/Debugger".to_string(),
            CopConfig {
                enabled: Some(false),
                exclude: vec![],
                include: vec![],
                severity: None,
                enforced_style: None,
                max: None,
                allow_safe_assignment: None,
                count_comments: None,
                raw: HashMap::new(),
            },
        );
        let cops = build_from_config(&cfg);
        assert!(
            cops.iter().all(|c| c.name() != "Lint/Debugger"),
            "disabled cop should not be built"
        );
    }

    #[test]
    fn build_one_returns_configured_cop() {
        let cfg = Config::default();
        let cop = build_one("Lint/Debugger", &cfg);
        assert!(cop.is_some(), "Lint/Debugger should be buildable");
        assert_eq!(cop.unwrap().name(), "Lint/Debugger");
    }
}
