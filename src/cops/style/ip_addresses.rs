//! Style/IpAddresses — flags hardcoded IP address strings.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use std::sync::OnceLock;

const MSG: &str = "Do not hardcode IP addresses.";
const IPV6_MAX_SIZE: usize = 45;

#[derive(Default)]
pub struct IpAddresses {
    allowed_addresses: Vec<String>,
}

impl IpAddresses {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(allowed_addresses: Vec<String>) -> Self {
        Self {
            allowed_addresses: allowed_addresses
                .into_iter()
                .map(|s| s.to_lowercase())
                .collect(),
        }
    }

    fn is_offense(&self, contents: &str) -> bool {
        if contents.is_empty() {
            return false;
        }
        if self.allowed_addresses.iter().any(|a| a == &contents.to_lowercase()) {
            return false;
        }
        if !potential_ip(contents) {
            return false;
        }
        ipv4_regex().is_match(contents) || ipv6_regex().is_match(contents)
    }
}

fn potential_ip(s: &str) -> bool {
    if s.len() > IPV6_MAX_SIZE {
        return false;
    }
    let first = match s.as_bytes().first() {
        Some(&b) => b,
        None => return false,
    };
    // Ruby's check: 48..58 (digits + ':'), 65..70 (A-F), 97..102 (a-f)
    (48..=58).contains(&first) || (65..=70).contains(&first) || (97..=102).contains(&first)
}

fn ipv4_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Ruby Resolv::IPv4::Regex: anchored full-string match.
        Regex::new(r"^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$").unwrap()
    })
}

fn ipv6_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Approximation of Resolv::IPv6::Regex covering common forms:
        // - full 8-group: a:b:c:d:e:f:g:h
        // - "::" compressed forms (zero or more groups on each side)
        // - IPv4-mapped: ::ffff:1.2.3.4
        let h16 = "[0-9A-Fa-f]{1,4}";
        let ipv4 = r"(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)";
        let pattern = format!(
            r"^(?:(?:{h16}:){{7}}{h16}|(?:{h16}:){{1,7}}:|(?:{h16}:){{1,6}}:{h16}|(?:{h16}:){{1,5}}(?::{h16}){{1,2}}|(?:{h16}:){{1,4}}(?::{h16}){{1,3}}|(?:{h16}:){{1,3}}(?::{h16}){{1,4}}|(?:{h16}:){{1,2}}(?::{h16}){{1,5}}|{h16}:(?::{h16}){{1,6}}|:(?:(?::{h16}){{1,7}}|:)|(?:{h16}:){{6}}{ipv4}|::ffff:{ipv4})$",
            h16 = h16, ipv4 = ipv4
        );
        Regex::new(&pattern).unwrap()
    })
}

impl Cop for IpAddresses {
    fn name(&self) -> &'static str {
        "Style/IpAddresses"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_string(&self, node: &ruby_prism::StringNode, ctx: &CheckContext) -> Vec<Offense> {
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        let src = &ctx.source[start..end];
        // Need quoted string; strip first + last char.
        if src.len() < 2 {
            return vec![];
        }
        let contents = &src[1..src.len() - 1];
        if self.is_offense(contents) {
            return vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)];
        }
        vec![]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_addresses: Option<Vec<String>>,
}

crate::register_cop!("Style/IpAddresses", |cfg| {
    let c: Cfg = cfg.typed("Style/IpAddresses");
    let allowed = c.allowed_addresses.unwrap_or_else(|| vec!["::".to_string()]);
    Some(Box::new(IpAddresses::with_config(allowed)))
});
